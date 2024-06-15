use clap::Parser;
use fs_err as fs;
use serde::Deserialize;
use thiserror::Error;
use tracing::*;

use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

mod logging;

#[derive(Parser)]
struct Cli {
    mod_list: PathBuf,
    #[arg(long = "id")]
    user_id: u64,
    #[arg(long = "access-token")]
    oauth2_access_token: PathBuf,
}

static RE_MOD: OnceLock<regex::Regex> = OnceLock::new();
fn re_mod() -> &'static regex::Regex {
    RE_MOD.get_or_init(|| regex::Regex::new("^https://mod.io/g/drg/m/(?P<name_id>[^/#]+)(:?#(?P<mod_id>\\d+)(:?/(?P<modfile_id>\\d+))?)?$").unwrap())
}

#[derive(Debug, Error)]
enum ModCheckError {
    #[error("mod not found: <{url}>")]
    ModNotFound { url: String },
    #[error("mod.io error for <{url}>: {error}")]
    ModioError { url: String, error: reqwest::Error },
    #[error("ambiguous mod.io URL: <{url}>")]
    AmbiguousModUrl { url: String },
}

const MODIO_DRG_ID: u32 = 2475;

#[derive(Debug, Deserialize)]
struct Mods {
    data: Vec<Mod>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct Mod {
    id: u32,
    visible: u32,
    profile_url: String,
}

async fn fetch_mods_by_name(
    client: &reqwest::Client,
    user_id: u64,
    token: &str,
    url: &str,
) -> Result<Mods, reqwest::Error> {
    let name_id = re_mod().captures(url).unwrap().name("name_id").unwrap().as_str();
    let url = format!(
        "https://u-{user_id}.modapi.io/v1/games/{MODIO_DRG_ID}/mods?visible=1&name_id={name_id}"
    );
    let res =
        client.get(url).header("accept", "application/json").bearer_auth(token).send().await?;
    let mods: Mods = res.json().await?;
    Ok(mods)
}

async fn check_url(
    client: &reqwest::Client,
    user_id: u64,
    token: &str,
    url: &str,
) -> Result<Mod, ModCheckError> {
    let mut mods = match fetch_mods_by_name(&client, user_id, token, url).await {
        Ok(mods) => mods,
        Err(error) => {
            debug!(?error, "request failed for <{url}>");
            return Err(ModCheckError::ModioError { url: url.to_string(), error });
        }
    };

    let Some(r#mod) = mods.data.pop() else {
        return Err(ModCheckError::ModNotFound { url: url.to_string() });
    };

    if !mods.data.is_empty() {
        return Err(ModCheckError::AmbiguousModUrl { url: url.to_string() });
    }

    Ok(r#mod)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    logging::setup_logging();

    let cli = Cli::parse();
    assert!(cli.mod_list.exists(), "`{}` does not exist", cli.mod_list.display());
    assert!(
        cli.oauth2_access_token.exists(),
        "`{}` does not exist",
        cli.oauth2_access_token.display()
    );
    let token = fs::read_to_string(&cli.oauth2_access_token)?;
    let token = token.trim();

    let mod_list = fs::read_to_string(&cli.mod_list)?;
    let mut mod_list = mod_list.lines().filter(|url| re_mod().is_match(url)).collect::<Vec<_>>();
    mod_list.dedup();
    debug!("mods_list: {:#?}", mod_list);

    let mut errors = vec![];

    let client = reqwest::Client::new();

    const CHUNK_SIZE: usize = 30;
    const SLEEP_SECS: u64 = 60;
    for chunk in mod_list.chunks(CHUNK_SIZE) {
        
        for url in chunk {
            debug!("checking {url}...");
            match check_url(&client, cli.user_id, token, url).await {
                Ok(Mod { profile_url, .. }) => {
                    info!(profile_url, "OK");
                }
                Err(e) => {
                    error!(?e, "INVALID");
                    errors.push(e);
                    continue;
                }
            }
        }

        info!("sleeping 60 seconds to avoid rate-limit");

        if chunk.len() == CHUNK_SIZE {
            tokio::time::sleep(Duration::from_secs(SLEEP_SECS)).await;
        }
    }

    let mut out = fs::File::create("errors.log")?;
    for e in &errors {
        writeln!(&mut out, "{}", e)?;
    }

    Ok(())
}
