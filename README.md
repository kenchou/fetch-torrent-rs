# fetch-torrent-rs

A command-line tool that directly downloads torrent files (*.torrent) from some torrent download sites. No browser, no ads. 
Theoretically, any site that supports form submission for downloads is supported.

Usage:
```
Rust implementation of fetch-torrent

Usage: fetch-torrent-rs [OPTIONS] <url>

Arguments:
  <url>  URL to process

Options:
  -o, --output <FILE>  Output filename
      --proxy <PROXY>  Proxy. example: socks5://user:pass@host:port
  -v...                Set log level
  -h, --help           Print help
  -V, --version        Print version
```

Based on the original Python project [fetch-torrent](https://github.com/kenchou/fetch-torrent), this project is rewritten in Rust to build a single executable file.
