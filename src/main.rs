extern crate reqwest;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate graphql_client;
use graphql_client::*;
#[macro_use]
extern crate log;
extern crate env_logger;

use std::collections::HashMap;
use std::env;
use std::fs::create_dir_all;
use std::fs::File;
use std::io::prelude::*;
use std::iter::FromIterator;
use std::sync::mpsc;
use std::thread;
use std::time;
mod repository;
use repository::Label;
use repository::Language;
use repository::Repository;
use repository::ToJavascript;
use repository::Topic;

type URI = String;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/schema.graphql",
    query_path = "src/repositories.graphql",
    response_derives = "Debug"
)]
pub struct Repositories;

const GITHUB_API_URL: &str = "https://api.github.com/graphql";
const NUM_LANGUAGES: i64 = 10;
const AVATAR_SIZE: i64 = 80;
const NUM_REPOSITORIES_PER_REQUEST: i64 = 50;
const MIN_NUM_ISSUES: i64 = 10;
const NUM_REPOSITORIES: usize = 20;
const NUM_RETRIES: i64 = 100;
const LABELS: [&str; 27] = [
    "help wanted",
    "beginner",
    "beginners",
    "easy",
    "Good First Bug",
    "starter",
    "status: ideal-for-contribution",
    "low-hanging-fruit",
    "E-easy",
    "newbie",
    "easy fix",
    "easy-fix",
    "beginner friendly",
    "easy-pick",
    "Good for New Contributors",
    "first-timers-only",
    "contribution-starter",
    "good for beginner",
    "starter bug",
    "good-for-beginner",
    "your-first-pr",
    "first timers only",
    "first time contributor",
    "up-for-grabs",
    "good first issue",
    "Contribute: Good First Issue",
    "D - easy",
];

impl Repositories {
    fn create_query(
        num_results: i64,
        language: &str,
        cursor: Option<String>,
    ) -> impl serde::ser::Serialize {
        let query = "language:".to_string() + language + " stars:>=500 is:public archived:false";
        Repositories::build_query(repositories::Variables {
            num_results,
            query,
            cursor,
            labels: LABELS.into_iter().map(ToString::to_string).collect(),
            num_languages: NUM_LANGUAGES,
            avatar_size: AVATAR_SIZE,
        })
    }

    fn parse_repository(repo: Option<repositories::RepositoriesSearchNodes>) -> Option<Repository> {
        let repo = match repo {
            Some(repo) => match repo {
                repositories::RepositoriesSearchNodes::Repository(repo) => repo,
                _ => {
                    error!("Search result is not a Repository.");
                    return None;
                }
            },
            None => {
                error!("Search result is empty.");
                return None;
            }
        };

        let languages = repo
            .languages
            .expect("")
            .nodes
            .unwrap_or_default()
            .into_iter()
            .map(|lang| lang.expect("").name)
            .collect::<Vec<_>>();

        let issues = repo
            .issues
            .nodes
            .unwrap_or_default()
            .into_iter()
            .map(|issue| {
                issue
                    .expect("")
                    .labels
                    .expect("")
                    .nodes
                    .unwrap_or_default()
                    .into_iter()
                    .map(|label| label.expect(""))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        let topics = repo
            .repository_topics
            .nodes
            .unwrap_or_default()
            .into_iter()
            .map(|topic| -> Topic {
                let topic = topic.expect("");
                Topic {
                    name: topic.topic.name,
                    url: topic.url,
                }
            })
            .collect::<Vec<_>>();

        let mut labels: HashMap<String, Label> =
            HashMap::from_iter(LABELS.iter().cloned().map(|label| {
                (
                    String::from(label),
                    Label {
                        name: String::from(label),
                        count: 0,
                        color: String::new(),
                    },
                )
            }));

        for issue in issues.iter() {
            for label in issue.iter() {
                labels.entry(label.name.clone()).and_modify(|l| {
                    (*l).count += 1;
                    (*l).color = label.color.clone();
                });
            }
        }

        let mut labels: Vec<Label> = labels
            .into_iter()
            .filter(|(_, v)| v.count > 0)
            .map(|(_, v)| v)
            .collect();
        labels.sort_by(|a, b| b.count.cmp(&a.count));

        if repo.issues.total_count < MIN_NUM_ISSUES {
            debug!("Not enough issues");
            return None;
        }

        let avatar_url = match repo.owner.on {
            repositories::RepositoriesSearchNodesOnRepositoryOwnerOn::User(user) => user.avatar_url,
            repositories::RepositoriesSearchNodesOnRepositoryOwnerOn::Organization(
                organization,
            ) => organization.avatar_url,
        };

        Some(Repository {
            name_with_owner: repo.name_with_owner,
            url: repo.url,
            description: repo.description.expect(""),
            homepage_url: repo.homepage_url.unwrap_or_default(),
            avatar_url,
            num_forks: repo.fork_count,
            num_issues: repo.issues.total_count,
            num_pull_requests: repo.pull_requests.total_count,
            num_stars: repo.stargazers.total_count,
            topics,
            labels,
            issues: vec![],
            languages,
        })
    }

    fn parse_response(response_data: repositories::ResponseData, search_object: &mut SearchObject) {
        debug!(
            "num repositories: {}\n{:#?}",
            response_data.search.repository_count, response_data.rate_limit
        );

        search_object.cursor = response_data.search.page_info.end_cursor;

        let mut repositories = response_data
            .search
            .nodes
            .unwrap_or_default()
            .into_iter()
            .filter_map(Repositories::parse_repository)
            .collect::<Vec<_>>();
        search_object.repositories.append(&mut repositories);
    }
}

struct SearchObject {
    language: String,
    repositories: Vec<Repository>,
    cursor: Option<String>,
    timeout: f32,
}

fn get_repositories(mut search_object: &mut SearchObject, gh_token: &str) {
    // Sleeping with exponential backoff so we do not make GitHub angry.
    thread::sleep(time::Duration::from_secs(search_object.timeout as u64));

    let q = Repositories::create_query(
        NUM_REPOSITORIES_PER_REQUEST,
        &search_object.language,
        search_object.cursor.clone(),
    );
    let client = reqwest::Client::new();

    let mut res = match client
        .post(GITHUB_API_URL)
        .bearer_auth(gh_token)
        .json(&q)
        .send()
    {
        Ok(res) => res,
        Err(e) => {
            error!("Error during send: {}", e);
            return;
        }
    };

    if res.status() != 200 {
        error!("Status: {}", res.status());
        if res.status() == 403 {
            search_object.timeout = search_object.timeout.powf(1.2).max(1.1);
        }
        return;
    }

    let response_body: Response<repositories::ResponseData> = match res.json() {
        Ok(res) => res,
        Err(e) => {
            error!("Error during json parsing: {}", e);
            return;
        }
    };

    let response_data: repositories::ResponseData = match response_body.data {
        Some(x) => x,
        None => {
            error!(
                "No data found for {} and cursor: {}.",
                search_object.language,
                search_object.cursor.clone().unwrap_or_default()
            );
            return;
        }
    };
    Repositories::parse_response(response_data, &mut search_object);
}

fn get_all_repositories(mut language: Language, gh_token: String) -> String {
    let mut search_object = SearchObject {
        cursor: None,
        language: language.search_term.clone(),
        repositories: vec![],
        timeout: 10.0,
    };
    let mut len = 0;
    while len < NUM_RETRIES && search_object.repositories.len() < NUM_REPOSITORIES {
        get_repositories(&mut search_object, &gh_token);
        info!(
            "{}: [{} / {}]",
            language.display_name,
            search_object.repositories.len(),
            NUM_REPOSITORIES
        );
        len = len + 1;
    }
    language.repositories = search_object.repositories;
    language.to_javascript()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    let gh_token = match env::var("GITHUB_GRAPHQL_TOKEN") {
        Ok(t) => t,
        Err(e) => {
            error!("Error github api token not defined: {}", e);
            return Err(Box::new(e));
        }
    };

    let mut f = match File::open("languages.json") {
        Ok(f) => f,
        Err(e) => {
            error!("Error can not open language json file: {}", e);
            return Err(Box::new(e));
        }
    };

    let mut buffer = String::new();
    match f.read_to_string(&mut buffer) {
        Ok(_) => {}
        Err(e) => {
            error!("Error can not read language json file: {}", e);
            return Err(Box::new(e));
        }
    };

    let languages: Vec<Language> = serde_json::from_str(&buffer).unwrap();

    let (tx, rx) = mpsc::channel();
    languages.into_iter().for_each(|language| {
        let tx = mpsc::Sender::clone(&tx);
        let gh_token = gh_token.clone();
        thread::spawn(move || {
            let repositories = get_all_repositories(language, gh_token);
            tx.send(repositories).unwrap();
        });
    });
    drop(tx);

    let mut result = vec![];
    for language in rx {
        result.push(language);
    }
    // This is super hacky and relies on the display_name being the first value to be serialized.
    result.sort();

    match create_dir_all("frontend/src/generated") {
        Ok(_) => {}
        Err(e) => {
            error!("Error during directory creation: {}", e);
            return Err(Box::new(e));
        }
    }

    let mut buffer = File::create("frontend/src/generated/data.js").expect("");
    match write!(buffer, "export default [\n{}\n];", result.join(",\n")) {
        Ok(v) => Ok(v),
        Err(e) => {
            error!("Error during final write: {}", e);
            return Err(Box::new(e));
        }
    }
}
