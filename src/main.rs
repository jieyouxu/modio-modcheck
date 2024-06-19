use clap::Parser;
use console::{Style, Term};
use fs_err as fs;
use indicatif::{ProgressBar, ProgressStyle};
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

impl ModCheckError {
    fn url(&self) -> &str {
        match self {
            ModCheckError::ModNotFound { url } => url,
            ModCheckError::ModioError { url, .. } => url,
            ModCheckError::AmbiguousModUrl { url } => url,
        }
    }

    fn status_code(&self) -> Option<u32> {
        match self {
            ModCheckError::ModNotFound { .. } => Some(404),
            ModCheckError::ModioError { error, .. } => {
                error.status().map(|code| code.as_u16() as u32)
            }
            ModCheckError::AmbiguousModUrl { .. } => None,
        }
    }
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

fn fetch_mods_by_name(
    client: &reqwest::blocking::Client,
    user_id: u64,
    token: &str,
    url: &str,
) -> Result<Mods, reqwest::Error> {
    let name_id = re_mod().captures(url).unwrap().name("name_id").unwrap().as_str();
    let url = format!(
        "https://u-{user_id}.modapi.io/v1/games/{MODIO_DRG_ID}/mods?visible=1&name_id={name_id}"
    );
    let res = client.get(url).header("accept", "application/json").bearer_auth(token).send()?;
    let mods: Mods = res.json()?;
    Ok(mods)
}

fn check_url(
    client: &reqwest::blocking::Client,
    user_id: u64,
    token: &str,
    url: &str,
) -> Result<Mod, ModCheckError> {
    let mut mods = match fetch_mods_by_name(&client, user_id, token, url) {
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

fn main() -> anyhow::Result<()> {
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

    let client = reqwest::blocking::Client::new();

    let pb = ProgressBar::new(mod_list.len() as u64);
    pb.set_style(
        ProgressStyle::with_template(if Term::stdout().size().1 > 80 {
            "{prefix:>12.cyan.bold} {spinner:.blue} [{bar:57}] {pos}/{len} {wide_msg}"
        } else {
            "{prefix:>12.cyan.bold} {spinner:.blue} [{bar:57}] {pos}/{len}"
        })
        .unwrap(),
    );
    pb.set_prefix("Checking");
    pb.enable_steady_tick(Duration::from_millis(100));

    let cyan_bold = Style::new().cyan().bold();
    let blue = Style::new().blue();
    let red_bold = Style::new().red().bold();
    let yellow_bold = Style::new().yellow().bold();

    const CHUNK_SIZE: usize = 30;
    const SLEEP_SECS: u64 = 60;
    for chunk in mod_list.chunks(CHUNK_SIZE) {
        for url in chunk {
            debug!("checking {url}...");
            match check_url(&client, cli.user_id, token, url) {
                Ok(Mod { profile_url, .. }) => {
                    debug!(profile_url, "OK");
                }
                Err(e) => {
                    debug!(?e, "INVALID");

                    let status = e
                        .status_code()
                        .map(|code| code.to_string())
                        .unwrap_or_else(|| "-".to_string());
                    let url = e.url();

                    let line = format!(
                        "{:>12} {:>3} {}",
                        red_bold.apply_to("ERROR"),
                        yellow_bold.apply_to(status),
                        url,
                    );
                    pb.println(line);

                    errors.push(e);
                }
            }

            pb.inc(1);
        }

        debug!("sleeping 60 seconds to avoid rate-limit");

        if chunk.len() == CHUNK_SIZE {
            let line = format!(
                "{:>12} waiting {} to not trigger mod.io rate limit",
                cyan_bold.apply_to("INFO"),
                blue.apply_to("60 seconds")
            );
            pb.println(line);
            std::thread::sleep(Duration::from_secs(SLEEP_SECS));
        }
    }
    pb.finish_and_clear();

    let error_log = PathBuf::from("errors.log");

    eprintln!("check completed, writing log to `{}`", error_log.display());

    let mut out = fs::File::create(&error_log)?;
    for e in &errors {
        match e {
            ModCheckError::ModNotFound { url } => writeln!(&mut out, "ERROR {:<10} {url}", 404)?,
            ModCheckError::ModioError { url, error } => match error.status() {
                Some(code) => writeln!(&mut out, "ERROR {code:<10} {url}")?,
                None => writeln!(&mut out, "ERROR {:<10} {url}", "---")?,
            },
            ModCheckError::AmbiguousModUrl { url } => {
                writeln!(&mut out, "ERROR {:<10} {url}", "ambiguous")?
            }
        }
    }

    Ok(())
}
