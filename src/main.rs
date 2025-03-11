use std::collections::HashSet;
use std::hash::{BuildHasherDefault, DefaultHasher};

use axum::{
    Router,
    http::StatusCode,
    response::{Form, Html},
    routing::{any, get},
};
use clap::Parser;
use graphql_client::GraphQLQuery;
use octocrab::{Octocrab, params::repos::Commitish};
use once_cell::sync::{Lazy, OnceCell};
use serde::Deserialize;
use tokio::sync::RwLock;

#[derive(Debug, Parser)]
struct Cli {
    #[arg(long, env, hide_env_values = true)]
    token: String,

    #[arg(long, env, hide_env_values = true)]
    login: String,

    #[arg(long, env, hide_env_values = true)]
    listen: std::net::SocketAddr,

    #[arg(long, env, hide_env_values = true)]
    auth_token: String,
}

static PULL_REQUESTS: Lazy<RwLock<Vec<PullRequest>>> = Lazy::new(|| RwLock::new(vec![]));
static CLI_OPTIONS: OnceCell<Cli> = OnceCell::new();

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "./assets/schema.docs.graphql",
    query_path = "./assets/get_pull_requests_query.graphql",
    response_derives = "Debug"
)]
pub struct GetPullRequestsQuery;

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

    let octocrab = Octocrab::builder()
        .personal_token(CLI_OPTIONS.get().unwrap().token.to_string())
        .build()
        .unwrap();

    // TODO: RyotaKが「100以上のOrgに所属している人なんて居ない」っていった！
    let mut target_logins: Vec<_> = octocrab
        .current()
        .list_org_memberships_for_authenticated_user()
        .send()
        .await
        .unwrap()
        .items
        .into_iter()
        .map(|v| v.organization.login)
        .collect();

    target_logins.push(octocrab.current().user().await.unwrap().login);

    let mut pull_requests = vec![];

    'login: for login in target_logins {
        let mut repositories_cursor = "".to_string();

        loop {
            let request_body =
                GetPullRequestsQuery::build_query(get_pull_requests_query::Variables {
                    login: login.to_string(),
                    repositories_cursor: repositories_cursor.to_string(),
                });

            let response: Result<
                graphql_client::Response<get_pull_requests_query::ResponseData>,
                _,
            > = client
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

            let data = response.unwrap().data.unwrap();

            let Some(data) = data.repository_owner else {
                tracing::warn!("Failed to get repository_onwer. ignoring login {login}.");
                continue 'login;
            };

            for repo in data.repositories.nodes.unwrap() {
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
    }

    let title_set: HashSet<_, BuildHasherDefault<DefaultHasher>> =
        HashSet::from_iter(pull_requests.iter().map(|v| v.title.as_str()));

    tracing::info!(
        "{} pull-requests registered ({} unique titles)",
        pull_requests.len(),
        title_set.len(),
    );

    *PULL_REQUESTS.write().await = pull_requests;
}

#[derive(Debug, Clone, Deserialize)]
struct AutoMergeRequest {
    login: String,
    title: String,
    auth_token: String,
}

async fn merge(Form(request): Form<AutoMergeRequest>) -> Result<&'static str, StatusCode> {
    if CLI_OPTIONS.get().unwrap().auth_token != request.auth_token {
        return Err(StatusCode::IM_A_TEAPOT);
    }

    tracing::info!(
        "Merge Requests Received! {}: {}",
        request.login,
        request.title
    );

    tokio::spawn(async move {
        let octocrab = Octocrab::builder()
            .personal_token(CLI_OPTIONS.get().unwrap().token.to_string())
            .build()
            .unwrap();

        // TODO: RyotaKが100以上のOrgに所属している人なんて居ないっていった！
        let mut target_logins: Vec<_> = octocrab
            .current()
            .list_org_memberships_for_authenticated_user()
            .send()
            .await
            .unwrap()
            .items
            .into_iter()
            .map(|v| v.organization.login)
            .collect();

        target_logins.push(octocrab.current().user().await.unwrap().login);

        let prs = PULL_REQUESTS.read().await.clone();

        let prs = prs
            .iter()
            .filter(|pr| pr.login == request.login)
            .filter(|pr| pr.title == request.title);

        'pr_loop: for pr in prs {
            let (owner, repo) = pr.repository.as_str().split_once("/").unwrap();

            tracing::info!("Start {owner}/{repo} #{}", pr.number);

            let Ok(octo_pr) = octocrab.pulls(owner, repo).get(pr.number as u64).await else {
                tracing::warn!("Failed to get known PR");
                continue;
            };

            if pr.title != octo_pr.title.unwrap() {
                tracing::warn!("PR title mismatch, maybe updated");
                continue;
            }

            let head_sha = octo_pr.head.sha;

            let checks = octocrab
                .checks(owner, repo)
                .list_check_runs_for_git_ref(Commitish(head_sha.to_owned()))
                .send()
                .await
                .unwrap();

            if checks.check_runs.is_empty() {
                tracing::warn!("CI Doesn't configured");
                continue;
            }

            for check in checks.check_runs {
                let positive_statuses = ["success", "skipped", "neutral"];
                let conclusion = check.conclusion.as_ref().unwrap().as_str();

                if !positive_statuses.contains(&conclusion) {
                    tracing::warn!(
                        "CI '{}' ({}) is not ready ({})",
                        check.name,
                        check.id,
                        conclusion
                    );

                    continue 'pr_loop;
                }
            }

            match octocrab
                .pulls(owner, repo)
                .merge(pr.number as u64)
                .title("Auto merge! (merge-pr-in-all)")
                .sha(head_sha)
                .method(octocrab::params::pulls::MergeMethod::Merge)
                .send()
                .await
            {
                Ok(_) => tracing::info!("Ready for merge!"),
                Err(e) => tracing::error!("Error occured: {e:?}"),
            }
        }
    });

    Ok("ok")
}

async fn root() -> Html<&'static str> {
    Html("<h1>Merge PR In All!</h1>")
}

#[tokio::main]
async fn main() {
    let subscriber = tracing_subscriber::fmt().compact().finish();

    tracing::subscriber::set_global_default(subscriber).unwrap();

    CLI_OPTIONS.set(Cli::parse()).unwrap();

    tracing::info!("Initializing...");
    refresh_pull_request().await;

    tokio::spawn(async {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(600)).await;
            refresh_pull_request().await
        }
    });

    let app = Router::new()
        .route("/", get(root))
        .route("/merge", any(merge));

    let listener = tokio::net::TcpListener::bind(CLI_OPTIONS.get().unwrap().listen)
        .await
        .unwrap();

    tracing::info!("Serving...");
    axum::serve(listener, app).await.unwrap()
}
