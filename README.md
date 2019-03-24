# [contrib](http://gauger.io/contrib)

[![Build Status](https://travis-ci.com/devgg/gh-issues.svg?branch=master)](https://travis-ci.com/devgg/gh-issues)
![License](https://img.shields.io/github/license/devgg/contrib.svg)

Find friendly open source projects with issues labeled for beginners 🚀. Begin contributing immediately 💻.

# Contributing

To start the application two steps are necessary.
- Generate the repository data with [Rust](https://www.rust-lang.org/)
- Start the [Create React App](https://github.com/facebook/create-react-app) with npm

### Prerequisite

- Install Rust ([instructions on rust-lang.org](https://www.rust-lang.org/tools/install))
- Install Node.js and npm ([instructions on npmjs.org](https://docs.npmjs.com/downloading-and-installing-node-js-and-npm))
- Create a GitHub personal access token ([instructions on github.com](https://developer.github.com/v4/guides/forming-calls/#authenticating-with-graphql))

```shell
git clone <YOUR_FORK>
cd contrib/
```

### Run the Application

```shell
export GITHUB_GRAPHQL_TOKEN=<YOUR_TOKEN>
# Generate the data by querying GitHub and Wikipedia.
RUST_BACKTRACE=1 RUST_LOG="contrib=info" cargo run
```

Now we can run the frontend.

```shell
cd frontend/
npm install
npm start
```

The page should open automatically 🔥.
