use reqwest::header::{HeaderMap, HeaderValue};

pub fn build_browser_headers(chrome_version: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();

    headers.insert("Host", HeaderValue::from_static("www.youtube.com"));
    headers.insert("Connection", HeaderValue::from_static("keep-alive"));
    headers.insert("sec-ch-ua", HeaderValue::from_str(&format!("\"Chromium\";v=\"{0}\", \"Google Chrome\";v=\"{0}\"", chrome_version)).unwrap());
    headers.insert("sec-ch-ua-mobile", HeaderValue::from_static("?0"));
    headers.insert("sec-ch-ua-platform", HeaderValue::from_static("\"Windows\""));
    headers.insert("DNT", HeaderValue::from_static("1"));
    headers.insert("Upgrade-Insecure-Requests", HeaderValue::from_static("1"));
    headers.insert("User-Agent", HeaderValue::from_str(&format!("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{0}.0.0.0 Safari/537.36", chrome_version)).unwrap());
    headers.insert("Accept", HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8"));
    headers.insert("Sec-Fetch-Site", HeaderValue::from_static("none"));
    headers.insert("Sec-Fetch-Mode", HeaderValue::from_static("navigate"));
    headers.insert("Sec-Fetch-User", HeaderValue::from_static("?1"));
    headers.insert("Sec-Fetch-Dest", HeaderValue::from_static("document"));
    headers.insert("Accept-Language", HeaderValue::from_static("en-US,en;q=0.9"));

    headers
}
