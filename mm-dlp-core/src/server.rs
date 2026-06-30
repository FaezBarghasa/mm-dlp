use actix_web::{web, App, HttpServer, HttpResponse, Responder, post, get};
use serde::Deserialize;
use std::sync::Arc;
use surrealdb::engine::local::Mem;
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
struct StreamQuery {
    videoId: String,
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

#[get("/stream")]
async fn get_stream(query: web::Query<StreamQuery>, state: web::Data<Arc<ServerState>>) -> impl Responder {
    let url = format!("https://pipedapi.kavin.rocks/streams/{}", query.videoId);
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
            // Format response back as JSON array matching SurrealDB HTTP response format
            // SurrealDB's queries return results for each statement.
            // For simplicity, we can serialize the query response.
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
                        // Empty query result (e.g. no rows returned for this index)
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
    // Initialize SurrealDB memory storage
    let _ = DB.connect::<Mem>(()).await;
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
            .service(get_trending)
            .service(run_sql)
    })
    .bind(("127.0.0.1", port))?
    .run()
    .await
}
