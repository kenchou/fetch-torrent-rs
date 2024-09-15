use clap::{Arg, Command};
use log::{debug, info, LevelFilter};
use reqwest::{ClientBuilder, Proxy, redirect::Policy};
use scraper::{Html, Selector};
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use url::Url;
use html_escape::decode_html_entities;
use urlencoding::decode;
use regex::Regex;
#[tokio::main]
async fn main() {
    let matches = Command::new("mdt")
        .version("1.0")
        .about("Rust implementation of MDT")
        .arg(Arg::new("url").required(true).help("URL to process"))
        .arg(
            Arg::new("output")
                .short('o')
                .long("output")
                .value_name("FILE")
                .help("Output filename"),
        )
        .arg(
            Arg::new("proxy")
                .long("proxy")
                .value_name("PROXY")
                .help("Proxy. example: socks5://user:pass@host:port"),
        )
        .arg(
            Arg::new("verbose")
                .short('v')
                .action(clap::ArgAction::Count)
                .help("Set log level"),
        )
        .get_matches();
    let url = matches.get_one::<String>("url").unwrap();
    let output_filename = matches.get_one::<String>("output").map(|s| s.as_str());
    let proxy = matches.get_one::<String>("proxy").map(|s| s.as_str());
    let log_level = match matches.get_count("verbose") {
        0 => LevelFilter::Warn,
        1 => LevelFilter::Info,
        2 => LevelFilter::Debug,
        _ => LevelFilter::Trace,
    };
    env_logger::Builder::new()
        .filter(None, log_level)
        .init();
    if let Err(err) = post_form(url, output_filename, proxy).await {
        eprintln!("Error: {}", err);
    }
}
async fn post_form(
    url: &str,
    output_filename: Option<&str>,
    proxy: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let client_builder = ClientBuilder::new()
        .redirect(Policy::default())  // 默认的重定向策略，可以跟随重定向
        .cookie_store(true);
    let client = if let Some(proxy) = proxy {
        client_builder.proxy(Proxy::all(proxy)?).build()?
    } else {
        client_builder.build()?
    };
    let response = client.get(url)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
        .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,/;q=0.8")
        .header("Accept-Language", "en-US,en;q=0.5")
        .header("Connection", "keep-alive")
        .send().await?;
    let response_url = response.url().clone();
    info!("POST Form: request URL {}", url);
    info!("Response URL: {}", response_url);
    info!("Status Code: {}", response.status());
    info!("Response Headers: {:?}", response.headers());
    let body = response.text().await?;
    debug!("Page HTML:\n{}", body);  // 打印页面HTML内容
    let document = Html::parse_document(&body);
    let form_selector = Selector::parse("form").unwrap();
    let input_selector = Selector::parse("input[name]").unwrap();
    let forms: Vec<_> = document.select(&form_selector).collect();
    debug!("Found {} forms", forms.len());  // 打印找到的表单数量
    if forms.is_empty() {
        return Err("No form found".into());
    }
    let form = forms.first().unwrap();
    let form_action = form.value().attr("action").ok_or("No form action found")?;
    let next_url = Url::parse(&response_url.join(form_action)?.as_str())?;
    let method = form.value().attr("method").unwrap_or("get");
    info!("action: {}", next_url);
    info!("method: {}", method);
    let mut data = HashMap::new();
    for input in form.select(&input_selector) {
        if let Some(name) = input.value().attr("name") {
            data.insert(name.to_string(), input.value().attr("value").unwrap_or("").to_string());
        }
    }
    info!("params: {:?}", data);
    info!("======= Next Request ========");
    info!("next request: {}, params: {:?}", next_url, data);
    let response = if method.to_lowercase() == "get" {
        client.get(next_url)
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
            .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,/;q=0.8")
            .header("Accept-Language", "en-US,en;q=0.5")
            .header("Connection", "keep-alive")
            .query(&data)
            .send().await?
    } else {
        client.post(next_url)
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
            .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,/;q=0.8")
            .header("Accept-Language", "en-US,en;q=0.5")
            .header("Connection", "keep-alive")
            .form(&data)
            .send().await?
    };
    info!(
        "Response URL: {}, Status: {}, Headers: {:?}",
        response.url(),
        response.status(),
        response.headers()
    );
    let filename = if let Some(output_filename) = output_filename {
        output_filename.to_string()
    } else {
        let parsed_url = Url::parse(url)?;
        let url_path = parsed_url.path();
        let default_filename = Path::new(url_path)
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("output"))
            .to_string_lossy()
            .to_string();
        if let Some(content_disposition) = response.headers().get(reqwest::header::CONTENT_DISPOSITION) {
            let content_disposition_str = match content_disposition.to_str() {
                Ok(v) => v.to_string(),
                Err(_) => String::from_utf8_lossy(content_disposition.as_bytes()).to_string(),
            };
            // 使用正则表达式解析 Content-Disposition 头信息
            let re_quoted = Regex::new(r#"filename\*?=["](.*?)["](;|$)"#).unwrap();
            let re_unquoted = Regex::new(r#"filename\*?=(.*?)(;|$)"#).unwrap();

            if let Some(caps) = re_quoted.captures(&content_disposition_str).or_else(|| re_unquoted.captures(&content_disposition_str)) {
                let filename_encoded = caps.get(1).unwrap().as_str();
                let decoded_filename = decode(filename_encoded).unwrap_or_else(|_| "output".into()).to_string();
                decode_html_entities(&decoded_filename).to_string()
            } else {
                default_filename
            }
        } else {
            default_filename
        }
    };
    info!("Save to file {}", filename);
    let mut file = File::create(filename)?;
    file.write_all(&response.bytes().await?)?;
    Ok(())
}