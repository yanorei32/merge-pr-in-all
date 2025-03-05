use clap::Parser;
use graphql_client::{GraphQLQuery, Response};
use octocrab::{
    models::Repository,
    params::{repos::Commitish, State},
    Octocrab,
};
use once_cell::sync::{Lazy, OnceCell};
use tokio::sync::RwLock;
use tracing::info;

#[derive(Debug, Parser)]
struct Cli {
    #[arg(long, env, hide_env_values = true)]
    token: String,

    #[arg(long, env, hide_env_values = true)]
    login: String,
}

static PULL_REQUESTS: Lazy<RwLock<Vec<PullRequest>>> = Lazy::new(|| RwLock::new(vec![]));
static CLI_OPTIONS: OnceCell<Cli> = OnceCell::new();

type BigInt = String;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "./assets/schema.docs.graphql",
    query_path = "./assets/get_pull_requests_query.graphql",
    response_derives = "Debug"
)]
pub struct GetPullRequestsQuery;

async fn fetch_associated_repositories(o: &Octocrab) -> Vec<Repository> {
    let mut repositories = vec![];

    for page in 1.. {
        info!("Fetching repositories: {page}");

        let repos = o
            .current()
            .list_repos_for_authenticated_user()
            .sort("updated")
            .per_page(100)
            .page(page)
            .send()
            .await
            .unwrap();

        // for dynamically changed
        if repos.items.is_empty() {
            break;
        }

        let is_last = repos.next.is_none();

        repositories.extend(repos);

        if is_last {
            break;
        }
    }

    info!("{} Repositories collected!", repositories.len());

    repositories
}

#[derive(Debug, Clone)]
struct PullRequest {
    repository: String,
    number: i64,
    title: String,
    login: String,
}

async fn refresh_pull_request() {
    let client = reqwest::Client::builder()
        .user_agent("hyper-shadero-nekoneko/0.1.0")
        .build()
        .unwrap();

    let mut repositories_cursor = "".to_string();
    let mut pull_requests = vec![];

    loop {
        let request_body = GetPullRequestsQuery::build_query(get_pull_requests_query::Variables {
            login: CLI_OPTIONS.get().unwrap().login.to_string(),
            repositories_cursor: repositories_cursor.to_string(),
        });

        let response: Result<graphql_client::Response<get_pull_requests_query::ResponseData>, _> =
            client
                .post("https://api.github.com/graphql")
                .header(
                    reqwest::header::AUTHORIZATION,
                    format!("token {}", CLI_OPTIONS.get().unwrap().token),
                )
                .json(&request_body)
                .send()
                .await
                .unwrap()
                .json()
                .await;

        let data = response.unwrap().data.unwrap().user.unwrap();

        for repo in data
            .repositories
            .nodes
            .unwrap()
        {
            let repo = repo.unwrap();
            for pr in repo.pull_requests.nodes.unwrap() {
                let pr = pr.unwrap();

                pull_requests.push(PullRequest {
                    login: pr.author.unwrap().login,
                    title: pr.title,
                    repository: repo.name_with_owner.clone(),
                    number: pr.number,
                })
            }
        }

        if !data.repositories.page_info.has_next_page {
            break;
        }

        repositories_cursor = data.repositories.page_info.end_cursor.unwrap();
    }

    *PULL_REQUESTS.write().await = pull_requests;
}

#[tokio::main]
async fn main() {
    let subscriber = tracing_subscriber::fmt().compact().finish();

    tracing::subscriber::set_global_default(subscriber).unwrap();

    CLI_OPTIONS.set(Cli::parse()).unwrap();

    refresh_pull_request().await;

    tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        refresh_pull_request().await
    });

    // response.data;
    // println!("{response:#?}");

    //
    // let octocrab = Octocrab::builder()
    //     .personal_token(config.token)
    //     .build()
    //     .unwrap();
    //
    // let repositories = fetch_associated_repositories(&octocrab).await;
    //
    // for repo in &repositories {
    //     println!(" `- {}", repo.full_name.as_ref().unwrap());
    //
    //     let owner = repo.owner.as_ref().unwrap();
    //
    //     let prs = octocrab
    //         .pulls(&owner.login, &repo.name)
    //         .list()
    //         .state(State::Open)
    //         .send()
    //         .await
    //         .unwrap();
    //
    //     for pr in prs {
    //         let author = pr.user.unwrap().login;
    //
    //         let head_sha = pr.head.sha;
    //         let title = pr.title.unwrap();
    //
    //         let checks = octocrab
    //             .checks(&owner.login, &repo.name)
    //             .list_check_runs_for_git_ref(Commitish(head_sha.to_owned()))
    //             .send()
    //             .await
    //             .unwrap();
    //
    //         println!("    `- {title} {author}");
    //
    //         if checks.check_runs.is_empty() {
    //             println!("      `- CI、ないがち！ｗ");
    //         }
    //
    //         for check in checks.check_runs {
    //             println!(
    //                 "      `- {} ({}) {}",
    //                 check.name,
    //                 check.id,
    //                 check.conclusion.unwrap()
    //             );
    //         }
    //     }
    // }
}
