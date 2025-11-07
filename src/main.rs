use clap::{Arg, Command};
use html_escape::decode_html_entities;
use log::{debug, info, LevelFilter};
use regex::Regex;
use reqwest::{redirect::Policy, ClientBuilder, Proxy};
use scraper::{Html, Selector};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use url::Url;
use urlencoding::decode;
#[tokio::main]
async fn main() {
    let matches = Command::new(env!("CARGO_PKG_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .about("Rust implementation of fetch-torrent")
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
    env_logger::Builder::new().filter(None, log_level).init();

    if let Err(err) = post_form(url, output_filename, proxy).await {
        eprintln!("Error: {}", err);
    }
}
async fn post_form(
    url: &str,
    output_filename: Option<&str>,
    proxy: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Request URL {}", url);

    let client_builder = ClientBuilder::new()
        .redirect(Policy::default()) // 默认的重定向策略，可以跟随重定向
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
    debug!("Response URL: {}", response_url);
    debug!("Status Code: {}", response.status());
    debug!("Response Headers: {:?}", response.headers());

    let body = response.text().await?;
    debug!("Response Body:\n{}", body); // 打印页面HTML内容

    let document = Html::parse_document(&body);
    let form_selector = Selector::parse("form").unwrap();
    let input_selector = Selector::parse("input[name]").unwrap();
    let forms: Vec<_> = document.select(&form_selector).collect();
    debug!("Found {} forms", forms.len()); // 打印找到的表单数量
    if forms.is_empty() {
        return Err("No form found".into());
    }
    let form = forms.first().unwrap();
    let form_action = form.value().attr("action").ok_or("No form action found")?;
    let form_action = if form_action.is_empty() {
        "download.php"
    } else {
        form_action
    };
    
    let next_url = Url::parse(&response_url.join(form_action)?.as_str())?;
    let method = form.value().attr("method").unwrap_or("get");
    info!("action: {}", next_url);
    info!("method: {}", method);
    let mut data = HashMap::new();
    for input in form.select(&input_selector) {
        if let Some(name) = input.value().attr("name") {
            data.insert(
                name.to_string(),
                input.value().attr("value").unwrap_or("").to_string(),
            );
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
        if let Some(content_disposition) =
            response.headers().get(reqwest::header::CONTENT_DISPOSITION)
        {
            let content_disposition_str = match content_disposition.to_str() {
                Ok(v) => v.to_string(),
                Err(_) => String::from_utf8_lossy(content_disposition.as_bytes()).to_string(),
            };
            // 使用正则表达式解析 Content-Disposition 头信息
            let re_quoted = Regex::new(r#"filename\*?=["](.*?)["](;|$)"#).unwrap();
            let re_unquoted = Regex::new(r#"filename\*?=(.*?)(;|$)"#).unwrap();

            if let Some(caps) = re_quoted
                .captures(&content_disposition_str)
                .or_else(|| re_unquoted.captures(&content_disposition_str))
            {
                let filename_encoded = caps.get(1).unwrap().as_str();
                let decoded_filename = decode(filename_encoded)
                    .unwrap_or_else(|_| "output".into())
                    .to_string();
                decode_html_entities(&decoded_filename).to_string()
            } else {
                default_filename
            }
        } else {
            default_filename
        }
    };
    // 使用新的安全下载逻辑
    download_file_with_conflict_handling(filename, &response.bytes().await?).await?;
    Ok(())
}

// 检查两个文件内容是否相同
fn files_content_equal(path1: &Path, path2: &Path) -> io::Result<bool> {
    let contents1 = fs::read(path1)?;
    let contents2 = fs::read(path2)?;
    Ok(contents1 == contents2)
}

// 生成带序号的文件名
fn generate_unique_filename(original_path: &Path) -> PathBuf {
    if !original_path.exists() {
        return original_path.to_path_buf();
    }

    let stem = original_path.file_stem().unwrap_or_default().to_string_lossy();
    let extension = original_path.extension().map(|ext| format!(".{}", ext.to_string_lossy())).unwrap_or_default();
    let parent = original_path.parent().unwrap_or_else(|| Path::new("."));

    let mut counter = 1;
    loop {
        let new_filename = format!("{}_{}{}", stem, counter, extension);
        let new_path = parent.join(&new_filename);
        if !new_path.exists() {
            return new_path;
        }
        counter += 1;
    }
}

// 安全下载文件，处理同名文件的情况
async fn download_file_with_conflict_handling(filename: String, content: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let filepath = Path::new(&filename);
    
    // 如果文件不存在，直接下载
    if !filepath.exists() {
        let mut file = File::create(filepath)?;
        file.write_all(content)?;
        println!("File downloaded: {}", filename);
        return Ok(());
    }
    
    // 创建临时文件
    let temp_filename = format!("{}.tmp", filename);
    let temp_path = Path::new(&temp_filename);
    {
        let mut temp_file = File::create(temp_path)?;
        temp_file.write_all(content)?;
    }
    
    // 比较临时文件与已存在文件的内容
    if files_content_equal(filepath, temp_path)? {
        // 如果内容相同，删除临时文件并跳过
        fs::remove_file(temp_path)?;
        println!("File {} already exists with same content, skipping.", filename);
    } else {
        // 如果内容不同，将临时文件重命名为带序号的文件
        let unique_path = generate_unique_filename(filepath);
        fs::rename(temp_path, &unique_path)?;
        println!("File content differs. Saved as: {}", unique_path.display());
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;
    use std::io::Write;

    #[test]
    fn test_files_content_equal() {
        // 创建两个具有相同内容的临时文件
        let mut temp1 = NamedTempFile::new().unwrap();
        let mut temp2 = NamedTempFile::new().unwrap();
        
        let content = b"test content";
        temp1.write_all(content).unwrap();
        temp2.write_all(content).unwrap();
        
        assert!(files_content_equal(temp1.path(), temp2.path()).unwrap());
    }
    
    #[test]
    fn test_files_content_not_equal() {
        // 创建两个具有不同内容的临时文件
        let mut temp1 = NamedTempFile::new().unwrap();
        let mut temp2 = NamedTempFile::new().unwrap();
        
        temp1.write_all(b"content 1").unwrap();
        temp2.write_all(b"content 2").unwrap();
        
        assert!(!files_content_equal(temp1.path(), temp2.path()).unwrap());
    }
    
    #[test]
    fn test_generate_unique_filename() {
        // 创建一个临时文件进行测试
        let temp_file = NamedTempFile::new().unwrap();
        let temp_path = temp_file.path();
        
        // 如果文件不存在，应该返回原始路径
        let non_existent = Path::new("/non/existent/file.txt");
        assert_eq!(generate_unique_filename(non_existent), non_existent);
        
        // 对于已存在的文件，应该生成带序号的文件名
        let unique_name = generate_unique_filename(temp_path);
        assert_ne!(unique_name, temp_path.to_path_buf());
        
        // 文件名应该包含序号
        let unique_str = unique_name.to_string_lossy();
        let original_str = temp_path.file_name().unwrap().to_string_lossy();
        assert!(unique_str.contains(&original_str[..original_str.len()-4])); // 不包含扩展名
        assert!(unique_str.contains("_1") || unique_str.contains("_2")); // 包含序号
    }
}
