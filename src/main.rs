extern crate reqwest;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate graphql_client;
use graphql_client::*;

use std::fs::File;
use std::io::prelude::*;
use std::sync::mpsc;
use std::thread;
mod repository;
use repository::Language;
use repository::Repository;
use repository::ToJavascript;
use repository::Topic;

use std::collections::HashMap;
use std::iter::FromIterator;

type URI = String;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/schema.graphql",
    query_path = "src/repositories.graphql",
    response_derives = "Debug"
)]
pub struct Repositories;

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
            cursor, //cursor.to_string(),
            labels: LABELS.into_iter().map(ToString::to_string).collect(),
            num_languages: NUM_LANGUAGES,
        })
    }

    fn parse_repository(repo: Option<repositories::RepositoriesSearchNodes>) -> Option<Repository> {
        let repo = match repo {
            Some(repo) => match repo {
                repositories::RepositoriesSearchNodes::Repository(repo) => repo,
                _ => {
                    println!("Search result is not a Repository.");
                    return None;
                }
            },
            None => {
                println!("Search result is empty.");
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
                    .map(|label| label.expect("").name)
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

        let mut label_counts: HashMap<String, i64> =
            HashMap::from_iter(LABELS.iter().cloned().map(|label| (String::from(label), 0)));

        for issue in issues.iter() {
            for label in issue.iter() {
                label_counts.entry(label.clone()).and_modify(|l| *l += 1);
            }
        }

        let mut label_counts: Vec<(String, i64)> = label_counts
            .into_iter()
            .filter(|label_count| label_count.1 > 0)
            .collect();
        label_counts.sort_by(|&(_, a), &(_, b)| b.cmp(&a));

        if repo.issues.total_count < 10 {
            println!("Not enough issues");
            return None;
        }

        Some(Repository {
            name_with_owner: repo.name_with_owner,
            url: repo.url,
            description: repo.description.expect(""),
            num_forks: repo.fork_count,
            num_issues: repo.issues.total_count,
            num_pull_requests: repo.pull_requests.total_count,
            num_stars: repo.stargazers.total_count,
            topics,
            label_counts,
            issues: vec![],
            languages,
        })
    }

    fn parse_response(response_data: repositories::ResponseData, search_object: &mut SearchObject) {
        println!(
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

const GITHUB_API_URL: &str = "https://api.github.com/graphql";
const GITHUB_AUTH_TOKEN: &str = "11cd3b0cfcae28d4f8708e7c8ff5d3a1d15aed9c";
const NUM_LANGUAGES: i64 = 10;
const NUM_REPOSITORIES_PER_REQUEST: i64 = 15;
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

struct SearchObject {
    language: String,
    repositories: Vec<Repository>,
    cursor: Option<String>,
}

fn get_repositories(mut search_object: &mut SearchObject) {
    let q = Repositories::create_query(
        NUM_REPOSITORIES_PER_REQUEST,
        &search_object.language,
        search_object.cursor.clone(),
    );
    let client = reqwest::Client::new();

    let mut res = match client
        .post(GITHUB_API_URL)
        .bearer_auth(GITHUB_AUTH_TOKEN)
        .json(&q)
        .send()
    {
        Ok(res) => res,
        Err(e) => {
            println!("{}", e);
            return; // None;
        }
    };

    println!("Status: {}", res.status());

    let response_body: Response<repositories::ResponseData> = match res.json() {
        Ok(res) => res,
        Err(e) => {
            println!("{}", e);
            return; // None;
        }
    };

    let response_data: repositories::ResponseData = match response_body.data {
        Some(x) => x,
        None => {
            println!("No data found.");
            return; // None;
        }
    };
    //Some();
    Repositories::parse_response(response_data, &mut search_object);
}

fn get_all_repositories(mut language: Language) -> String {
    let mut search_object = SearchObject {
        cursor: None,
        language: language.search_term.clone(),
        repositories: vec![],
    };
    let mut len = 0;
    while len < NUM_RETRIES && search_object.repositories.len() < NUM_REPOSITORIES {
        get_repositories(&mut search_object);
        len = len + 1; //search_object.repositories.len();
    }
    language.repositories = search_object.repositories;
    language.to_javascript()
}

fn main() {
    let mut f = File::open("languages.json").unwrap();
    let mut buffer = String::new();
    f.read_to_string(&mut buffer).unwrap();
    let languages: Vec<Language> = serde_json::from_str(&buffer).unwrap();

    let (tx, rx) = mpsc::channel();
    languages.into_iter().for_each(|language| {
        let tx = mpsc::Sender::clone(&tx);
        thread::spawn(move || {
            let repositories = get_all_repositories(language);
            tx.send(repositories).unwrap();
            //drop(tx);
        });
    });
    drop(tx);

    let mut result = vec![];
    for language in rx {
        result.push(language);
    }

    let mut buffer = File::create("frontend/src/generated/data.js").expect("");
    match write!(buffer, "export default [\n{}\n];", result.join(",\n")) {
        Ok(_) => return,
        Err(e) => {
            println!("{}", e);
        }
    }
}
