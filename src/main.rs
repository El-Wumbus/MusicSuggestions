#![feature(result_option_map_or_default)]
use ahash::RandomState;
use eyre::Context;
use serde::Deserialize;
use std::{collections::HashMap, fmt::Debug, fs, io::Read, path::Path};
use tiny_http::{Header, Method, Request, Response};
use uri_rs::UriOwned;

mod page;
#[macro_use]
mod macros;

pub const CSS: &str = include_str!("../styles.css");
pub const NAME: &str = env!("CARGO_CRATE_NAME");

#[derive(Debug, Deserialize)]
struct Config {
    bind: Option<String>,
}

fn load_config(path: impl AsRef<Path>) -> eyre::Result<Config> {
    let path = path.as_ref();
    let contents =
        fs::read_to_string(path).context(format!("Failed to read config from {path:?}"))?;
    let config =
        toml::from_str(&contents).context(format!("Failed to parse TOML from {path:?}"))?;
    Ok(config)
}

fn main() -> eyre::Result<()> {
    env_logger::builder().init();
    let config_dir = dirs::config_dir()
        .expect("System should have a config directory")
        .join(NAME);
    let config_path = config_dir.join("config.toml");
    let config = load_config(&config_path)?;
    let bind = config.bind.unwrap_or_else(|| "0.0.0.0:8000".to_string());

    let rstate = RandomState::default();
    let mut cache: HashMap<u64, String, RandomState> = HashMap::default();
    let music_releases = page::music::prepare()?;

    let caching_headers: &[Header] = &[
        "Content-Type: text/html".parse().unwrap(),
        "Cache-Control: public, max-age=900".parse().unwrap(),
    ];
    let server = tiny_http::Server::http(bind).unwrap();
    loop {
        let mut request = match server.recv() {
            Ok(rq) => rq,
            Err(e) => {
                eprintln!("error: {e}");
                break;
            }
        };
        let method = request.method().to_owned();
        let headers = request.headers();
        let url = request.url();
        let Ok(url) = UriOwned::new(url) else {
            let _ = request.respond(Response::new_empty(tiny_http::StatusCode(404)));
            continue;
        };
        let Some(mut path) = url.path.as_deref() else {
            let _ = request.respond(Response::new_empty(tiny_http::StatusCode(404)));
            continue;
        };
        if path.ends_with('/') && path != "/" {
            path = &path[0..path.len() - 1];
        }

        let query = url.as_ref().get_query_parameters().unwrap_or_default();

        let key = {
            let mut key: Vec<u8> = vec![];
            key.extend_from_slice(method.as_str().as_bytes());
            // Whitelist headers to include
            headers
                .iter()
                .filter(|_| false)
                .map(|x| (x.field.as_str().as_bytes(), x.value.as_str().as_bytes()))
                .for_each(|(k, v)| {
                    key.extend_from_slice(k);
                    key.extend_from_slice(v);
                });
            key.extend_from_slice(request.url().as_bytes());
            let body = {
                let mut body = vec![];
                request.as_reader().read_to_end(&mut body)?;
                body
            };
            key.extend_from_slice(&body);
            rstate.hash_one(key)
        };

        let mut response = match (method, path) {
            (Method::Get, "/") => Response::empty(308)
                .with_header(Header::from_bytes(b"Location", page::music::PATH.as_bytes()).unwrap())
                .boxed(),
            (Method::Get, "/music") => {
                let html = cache.get(&key).cloned().unwrap_or_else(|| {
                    let v = page::music::render(&music_releases, &query);
                    cache.insert(key, v.clone());
                    v
                });
                Response::from_string(html).boxed()
            }
            (Method::Get, "/words") => page::words::render(&query),
            _ => {
                eprintln!("Couldn't find {path:?}");
                Response::new_empty(tiny_http::StatusCode(404)).boxed()
            }
        };
        for header in caching_headers.iter() {
            response.add_header(header.clone());
        }
        respond_or_complain(request, response);
    }

    Ok(())
}

fn respond_or_complain<R: Read>(req: Request, response: Response<R>) {
    if let Err(e) = req.respond(response) {
        eprintln!("Failed to respond: {e}");
    }
}
