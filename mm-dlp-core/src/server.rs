use actix_web::{web, App, HttpServer, HttpResponse, Responder, post, get};
use serde::Deserialize;
use std::sync::Arc;
use surrealdb::engine::local::{Mem, SurrealKv};
use surrealdb::Surreal;

lazy_static::lazy_static! {
    static ref DB: Surreal<surrealdb::engine::local::Db> = Surreal::init();
}

pub struct ServerState {
    pub client: reqwest::Client,
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
}

#[derive(Deserialize)]
struct SuggestionQuery {
    query: String,
}

#[get("/search")]
async fn search_music(query: web::Query<SearchQuery>, state: web::Data<Arc<ServerState>>) -> impl Responder {
    let url = format!("https://pipedapi.kavin.rocks/search?q={}&filter=music_songs", query.q);
    match state.client.get(&url).send().await {
        Ok(res) => {
            if let Ok(body) = res.text().await {
                HttpResponse::Ok().content_type("application/json").body(body)
            } else {
                HttpResponse::InternalServerError().body("Failed to read search response")
            }
        }
        Err(e) => HttpResponse::BadGateway().body(e.to_string()),
    }
}

#[get("/streams/{videoId}")]
async fn get_stream(path: web::Path<String>, state: web::Data<Arc<ServerState>>) -> impl Responder {
    let video_id = path.into_inner();
    let url = format!("https://pipedapi.kavin.rocks/streams/{}", video_id);
    match state.client.get(&url).send().await {
        Ok(res) => {
            if let Ok(body) = res.text().await {
                HttpResponse::Ok().content_type("application/json").body(body)
            } else {
                HttpResponse::InternalServerError().body("Failed to read stream response")
            }
        }
        Err(e) => HttpResponse::BadGateway().body(e.to_string()),
    }
}

#[get("/suggestions")]
async fn get_suggestions(query: web::Query<SuggestionQuery>, state: web::Data<Arc<ServerState>>) -> impl Responder {
    let url = format!("https://pipedapi.kavin.rocks/suggestions?query={}", query.query);
    match state.client.get(&url).send().await {
        Ok(res) => {
            if let Ok(body) = res.text().await {
                HttpResponse::Ok().content_type("application/json").body(body)
            } else {
                HttpResponse::InternalServerError().body("Failed to read suggestions response")
            }
        }
        Err(e) => HttpResponse::BadGateway().body(e.to_string()),
    }
}

#[get("/playlists/{playlistId}")]
async fn get_playlist(path: web::Path<String>, state: web::Data<Arc<ServerState>>) -> impl Responder {
    let playlist_id = path.into_inner();
    let url = format!("https://pipedapi.kavin.rocks/playlists/{}", playlist_id);
    match state.client.get(&url).send().await {
        Ok(res) => {
            if let Ok(body) = res.text().await {
                HttpResponse::Ok().content_type("application/json").body(body)
            } else {
                HttpResponse::InternalServerError().body("Failed to read playlist response")
            }
        }
        Err(e) => HttpResponse::BadGateway().body(e.to_string()),
    }
}

#[get("/channel/{channelId}")]
async fn get_channel(path: web::Path<String>, state: web::Data<Arc<ServerState>>) -> impl Responder {
    let channel_id = path.into_inner();
    let url = format!("https://pipedapi.kavin.rocks/channel/{}", channel_id);
    match state.client.get(&url).send().await {
        Ok(res) => {
            if let Ok(body) = res.text().await {
                HttpResponse::Ok().content_type("application/json").body(body)
            } else {
                HttpResponse::InternalServerError().body("Failed to read channel response")
            }
        }
        Err(e) => HttpResponse::BadGateway().body(e.to_string()),
    }
}

#[get("/trending")]
async fn get_trending(state: web::Data<Arc<ServerState>>) -> impl Responder {
    let url = "https://pipedapi.kavin.rocks/trending?region=US";
    match state.client.get(url).send().await {
        Ok(res) => {
            if let Ok(body) = res.text().await {
                HttpResponse::Ok().content_type("application/json").body(body)
            } else {
                HttpResponse::InternalServerError().body("Failed to read trending response")
            }
        }
        Err(e) => HttpResponse::BadGateway().body(e.to_string()),
    }
}

#[post("/sql")]
async fn run_sql(body: String) -> impl Responder {
    match DB.query(&body).await {
        Ok(mut response) => {
            let mut json_results = Vec::new();
            let mut i: usize = 0;
            loop {
                match response.take::<Option<serde_json::Value>>(i) {
                    Ok(Some(val)) => {
                        json_results.push(serde_json::json!({
                            "status": "OK",
                            "time": "1ms",
                            "result": val
                        }));
                        i += 1;
                    }
                    Ok(None) => {
                        json_results.push(serde_json::json!({
                            "status": "OK",
                            "time": "1ms",
                            "result": []
                        }));
                        i += 1;
                    }
                    Err(_) => break,
                }
            }
            HttpResponse::Ok().json(json_results)
        }
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!([{
            "status": "ERR",
            "detail": e.to_string()
        }])),
    }
}

pub async fn start_server(port: u16) -> std::io::Result<()> {
    let path = std::env::var("M_SCRAPER_DB_PATH").unwrap_or_else(|_| "memory".to_string());
    if path == "memory" {
        let _ = DB.connect::<Mem>(()).await;
        println!("Connected to SurrealDB embedded in-memory instance.");
    } else {
        let _ = DB.connect::<SurrealKv>(&path).await;
        println!("Connected to SurrealDB embedded file instance at: {}", path);
    }
    let _ = DB.use_ns("m-scraper").use_db("music").await;

    let state = Arc::new(ServerState {
        client: reqwest::Client::new(),
    });

    println!("Starting local server on port {}", port);
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(state.clone()))
            .service(search_music)
            .service(get_stream)
            .service(get_suggestions)
            .service(get_playlist)
            .service(get_channel)
            .service(get_trending)
            .service(run_sql)
    })
    .bind(("127.0.0.1", port))?
    .run()
    .await
}
