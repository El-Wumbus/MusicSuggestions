use crate::{CSS, NAME, page::nav::NAVBAR};
use eyre::Context;
use musicbrainz_rs::{
    MusicBrainzClient,
    chrono::NaiveDate,
    entity::{CoverartResponse, date_string::DateString, release_group::ReleaseGroup},
    prelude::*,
};
use serde::{Deserialize, Serialize};
use std::{
    fmt::Write,
    fs,
    io::Write as _,
    path::Path,
    time::{Duration, Instant},
};
use uri_rs::QueryParameters;

pub const PATH: &str = "/music";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Release {
    /// `MusicBrainz` `ReleaseGroupID`
    pub rgid:   String,
    /// Whether this is highly recomended
    pub highly: bool,

    pub title:         String,
    pub artwork:       Option<String>,
    pub release_date:  Option<DateString>,
    pub artist_credit: Option<String>,
    pub genres:        Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub recs: Vec<Recommendation>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Cache {
    #[serde(default)]
    pub releases: Vec<Release>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Recommendation {
    // Release Group MBID
    pub release: String,
    #[serde(default)]
    pub highly:  bool,
}

pub fn prepare() -> eyre::Result<Vec<Release>> {
    let cache_path = dirs::cache_dir()
        .expect("System should have a cache directory")
        .join(NAME)
        .join("music.toml");
    let config_path = dirs::config_dir()
        .expect("System should have a config directory")
        .join(NAME)
        .join("music.toml");
    let config = load_config(&config_path)?;
    let cache = load_cache(&cache_path, config)?;
    Ok(cache)
}

fn load_config(path: impl AsRef<Path>) -> eyre::Result<Config> {
    let path = path.as_ref();
    let contents =
        std::fs::read_to_string(path).context(format!("Failed to read {path:?} to string"))?;
    let config = toml::from_str(&contents)?;
    Ok(config)
}

fn load_cache(path: impl AsRef<Path>, config: Config) -> eyre::Result<Vec<Release>> {
    let mut err = None;
    let path = path.as_ref();
    let mut releases: Vec<Release> = Vec::new();
    let mut client = MusicBrainzClient::default();

    client.max_retries = 3;
    client.set_user_agent("Decator's Music Suggestions 0.1.0 (expo-plusplus@proton.me)")?;

    if path.exists() {
        let contents = fs::read_to_string(path)?;
        let contents: Cache = toml::from_str(&contents)?;
        releases = contents.releases;
    }

    releases.retain(|cached| config.recs.iter().any(|rec| rec.release == cached.rgid));
    let mut last_fetch = Instant::now();
    'outer: for rec in &config.recs {
        for release in &mut releases {
            if release.rgid == rec.release {
                release.highly = rec.highly;
                continue 'outer;
            }
        }
        let now = Instant::now();
        if now - last_fetch < Duration::from_secs(4) {
            eprintln!("Waiting for rate limit...");
            std::thread::sleep(now - last_fetch);
        }

        let rg = match get_releasegroup(&client, &rec.release) {
            Ok(r) => r,
            Err(e) => {
                err = Some(e);
                break;
            }
        };
        eprintln!("Waiting for rate limit...");
        std::thread::sleep(Duration::from_secs(4));
        let artwork = match get_releasegroup_image(&client, &rec.release) {
            Ok(a) => a,
            Err(e) => {
                err = Some(e);
                break;
            }
        };
        last_fetch = Instant::now();

        let release = Release {
            rgid: rec.release.clone(),
            highly: rec.highly,
            title: rg.title,
            artwork,
            release_date: rg.first_release_date,
            artist_credit: rg
                .artist_credit
                .into_iter()
                .flatten()
                .map(|x| x.name)
                .next(),
            genres: rg.genres.into_iter().flatten().map(|x| x.name).collect(),
        };
        releases.push(release);
    }
    releases.sort_by(|a, b| a.title.cmp(&b.title));

    let cache = Cache { releases };
    let contents = toml::to_string(&cache)?;

    let parent = path.parent().expect("this is a file");
    if !parent.is_dir() {
        fs::create_dir_all(parent).context(format!(
            "Failed to create parent directory of cache file: {parent:?}"
        ))?;
    }
    let mut f = fs::File::create(path)?;
    f.write_all(contents.as_bytes())?;

    if let Some(err) = err {
        return Err(err.into());
    }
    eprintln!("I have {} releases!", cache.releases.len());
    Ok(cache.releases)
}

pub fn render(releases: &[Release], query: &QueryParameters) -> String {
    let mut releases: Vec<_> = releases.iter().collect();
    if let Some(Some(sort)) = query.get("sort") {
        match sort.as_str() {
            "title" => {
                releases.sort_by(|a, b| a.title.cmp(&b.title));
            }
            "artist" => {
                releases.sort_by(|a, b| a.artist_credit.cmp(&b.artist_credit));
            }
            "release_date" => {
                // HOLY stupid
                releases.sort_by(|a, b| {
                    b.release_date
                        .as_ref()
                        .map(|x| x.into_naive_date(1, 1, 1).unwrap_or(NaiveDate::MIN))
                        .cmp(
                            &a.release_date
                                .as_ref()
                                .map(|x| x.into_naive_date(1, 1, 1).unwrap_or(NaiveDate::MIN)),
                        )
                });
            }
            _ => {}
        }
    }

    generate_html(&releases)
}

fn generate_html(releases: &[&Release]) -> String {
    const TITLE: &str = "Recommendations";
    let mut buf = String::new();
    writeln!(buf, "<!DOCTYPE html>").unwrap();
    writeln!(buf, r#"<html lang="en-US">"#).unwrap();
    writeln!(
        buf,
        r#"
         <head>
         <meta charset="utf-8" />
         <meta name="viewport" content="width=device-width, initial-scale=1">
         <title>{TITLE}</title>
         <meta property="og:title" content="{TITLE}" />
         <style>
         {CSS}
         </style>
         </head>
    "#
    )
    .unwrap();

    buf.write_str(NAVBAR.as_str()).unwrap();
    writeln!(
        buf,
        r#"<div id="music-page-contents">{}</div>"#,
        generate_body(releases)
    )
    .unwrap();
    writeln!(buf, r"</html>").unwrap();

    buf
}

fn generate_body(releases: &[&Release]) -> String {
    let highly_recommended = releases.iter().filter(|r| r.highly).collect::<Vec<_>>();
    let n_recommended = releases.iter().filter(|r| !r.highly).collect::<Vec<_>>();
    let mut buf = String::new();

    writeln!(buf, r#"<div class="music-nav">"#).unwrap();
    writeln!(
        buf,
        r#"<div class="label" style="display: inline-block">Sort by:</div>"#
    )
    .unwrap();
    writeln!(
        buf,
        r#"<a class="button" href="{PATH}/?sort=artist">Artist</a>"#
    )
    .unwrap();
    writeln!(
        buf,
        r#"<a class="button" href="{PATH}/?sort=title">Album</a>"#
    )
    .unwrap();
    writeln!(
        buf,
        r#"<a class="button" href="{PATH}/?sort=release_date">Release Date (Decending)</a>"#
    )
    .unwrap();
    writeln!(buf, "</div>").unwrap();

    writeln!(buf, "<h2>Highly Recommended</h2>").unwrap();
    writeln!(
        buf,
        r#"<ul id="highly-recommended" class="recommendation-list">"#
    )
    .unwrap();
    {
        for release in highly_recommended {
            write!(buf, "{}", generate_release_element(release)).unwrap();
        }
    }
    writeln!(buf, "</ul>").unwrap();

    writeln!(buf, "<h2>Recommended</h2>").unwrap();
    writeln!(buf, r#"<ul id="recommended" class="recommendation-list">"#).unwrap();
    {
        for release in n_recommended {
            write!(buf, "{}", generate_release_element(release)).unwrap();
        }
    }
    writeln!(buf, "</ul>").unwrap();
    buf
}

fn generate_release_element(release: &Release) -> String {
    let mut buf = String::new();
    writeln!(buf, "<li>").unwrap();
    writeln!(buf, r#"<div class="album-grid-container">"#).unwrap();
    if let Some(img) = release.artwork.as_deref() {
        writeln!(buf, r#"<img src="{img}" />"#).unwrap();
    }

    writeln!(buf, r#"<div class="album-grid-info">"#).unwrap();
    {
        writeln!(
            buf,
            r#"<div class="label"><strong>MBID:</strong></div><a style="hyphens: manual; overflow-wrap: anywhere;" href=https://musicbrainz.org/release-group/{mbid}>{mbid}</a>"#,
            mbid = release.rgid
        )
        .unwrap();
        if let Some(artist) = release.artist_credit.as_deref() {
            writeln!(
                buf,
                r#"<div class="label"><strong>Artist:</strong></div><div>{artist}</div>"#
            )
            .unwrap();
        }
        writeln!(
            buf,
            r#"<div class="label"><strong>Album:</strong></div><div>{title}</div>"#,
            title = release.title
        )
        .unwrap();
        if let Some(DateString(release_date)) = release.release_date.as_ref() {
            writeln!(
                buf,
                r#"<div class="label"><strong>Release Date:</strong></div><div>{release_date}</div>"#
            )
            .unwrap();
        }
        writeln!(
            buf,
            r#"<div class="label"><strong>Genres:</strong></div><div>{genres}</div>"#,
            genres = release.genres.join(", ")
        )
        .unwrap();
    }
    writeln!(buf, "</div>").unwrap();

    writeln!(buf, "</div>").unwrap();
    writeln!(buf, "</li>").unwrap();
    buf
}

/// Try up to three times to get the release group.
fn get_releasegroup(
    client: &MusicBrainzClient,
    id: &str,
) -> Result<ReleaseGroup, musicbrainz_rs::Error> {
    let mut tries = 3i32;
    loop {
        eprintln!("Getting info for: {id:?}...");
        let attempt = ReleaseGroup::fetch()
            .id(id)
            .with_artists()
            .with_genres()
            .execute_with_client(client);

        match attempt {
            Ok(rg) => {
                break Ok(rg);
            }
            Err(musicbrainz_rs::Error::ReqwestError(e))
                if e.status()
                    .is_some_and(|x| x == reqwest::StatusCode::SERVICE_UNAVAILABLE)
                    || e.is_request() && !e.is_status() =>
            {
                if tries < 0 {
                    break Err(musicbrainz_rs::Error::ReqwestError(e));
                }
                tries -= 1;
                std::thread::sleep(Duration::from_secs(4));
            }
            Err(e) => break Err(e),
        }
    }
}

fn get_releasegroup_image(
    client: &MusicBrainzClient,
    id: &str,
) -> Result<Option<String>, musicbrainz_rs::Error> {
    let mut tries = 3i32;
    loop {
        eprintln!("Getting image for: {id:?}...");
        let attempt = ReleaseGroup::fetch_coverart()
            .id(id)
            .front()
            .res_250()
            .execute_with_client(client);
        match attempt {
            Ok(img) => {
                break Ok(match img {
                    CoverartResponse::Url(x) => Some(x),
                    CoverartResponse::Json(coverart) => coverart
                        .images
                        .into_iter()
                        .filter(|x| x.front)
                        .map(|x| x.image)
                        .next(),
                });
            }
            Err(musicbrainz_rs::Error::ReqwestError(e))
                if e.status()
                    .is_some_and(|x| x == reqwest::StatusCode::SERVICE_UNAVAILABLE)
                    || e.is_request() && !e.is_status() =>
            {
                if tries < 0 {
                    break Err(musicbrainz_rs::Error::ReqwestError(e));
                }
                tries -= 1;
                std::thread::sleep(Duration::from_secs(4));
            }
            Err(e) => break Err(e),
        }
    }
}
