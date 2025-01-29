use actix_web::{web, App, HttpServer, Responder, HttpResponse};
use actix_files; // Для обслуживания статических файлов
use clap::Parser;
use serde::Deserialize;
use sysinfo::{Networks, System}; // Для работы с сетевым трафиком
use tokio_postgres::{NoTls, Error}; // Для работы с PostgreSQL
use serde_json; // Для работы с JSON
use tokio::time::sleep; // Для асинхронного ожидания
use std::sync::{Arc, Mutex};
use std::env; // Для работы с переменными окружения

// Шаг 1: Определяем CLI-аргументы
#[derive(Parser)]
#[command(name = "Network Farming")]
#[command(author = "Your Name <your.email@example.com>")]
#[command(version = "1.0")]
#[command(about = "Farms points from unused network traffic", long_about = None)]
struct Cli {
    #[arg(short, long, default_value_t = 1024)]
    threshold: u64,
}

// Шаг 2: Определяем конфигурацию ноды
#[derive(Deserialize)]
struct NodeConfig {
    threshold: u64,
}

// Шаг 3: Структура для хранения данных о сетевом трафике
#[derive(Debug)]
struct NetworkUsage {
    sent: u64,
    received: u64,
}

impl NetworkUsage {
    fn new(networks: &Networks) -> Self {
        let mut total_sent = 0;
        let mut total_received = 0;

        for (_interface_name, network) in networks.iter() {
            total_sent += network.total_transmitted();
            total_received += network.total_received();
        }

        NetworkUsage {
            sent: total_sent,
            received: total_received,
        }
    }

    fn get_unused_bandwidth(&self, previous: &NetworkUsage) -> u64 {
        let current_total = self.sent + self.received;
        let previous_total = previous.sent + previous.received;
        if current_total > previous_total {
            current_total - previous_total
        } else {
            0
        }
    }
}

// Шаг 4: Подключение к базе данных PostgreSQL
async fn connect_to_db() -> Result<tokio_postgres::Client, Error> {
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let (client, connection) = tokio_postgres::connect(&database_url, NoTls).await?;

    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("Connection error: {}", e);
        }
    });

    client
        .execute(
            "CREATE TABLE IF NOT EXISTS users (
                id SERIAL PRIMARY KEY,
                username TEXT NOT NULL UNIQUE,
                points BIGINT NOT NULL
            )",
            &[],
        )
        .await?;

    client
        .execute(
            "CREATE TABLE IF NOT EXISTS global_stats (
                id SERIAL PRIMARY KEY,
                total_points BIGINT NOT NULL
            )",
            &[],
        )
        .await?;

    let row_count: i64 = client
        .query_one("SELECT COUNT(*) FROM global_stats", &[])
        .await?
        .get(0);
    if row_count == 0 {
        client
            .execute("INSERT INTO global_stats (total_points) VALUES (0)", &[])
            .await?;
    }

    Ok(client)
}

// Шаг 5: Добавление пользователя в базу данных
async fn add_user(client: &tokio_postgres::Client, username: &str, points: i64) -> Result<(), Error> {
    // Используем ON CONFLICT для игнорирования дубликатов
    client
        .execute(
            "INSERT INTO users (username, points) VALUES ($1, $2) ON CONFLICT (username) DO NOTHING",
            &[&username, &points],
        )
        .await?;

    println!("Attempted to add user '{}'.", username);
    Ok(())
}

// Шаг 6: Мониторинг сетевого трафика и начисление поинтов
async fn monitor_network(client: Arc<tokio_postgres::Client>, config: Arc<Mutex<NodeConfig>>) {
    let _system = System::new_all();
    let mut networks = Networks::new_with_refreshed_list();
    let mut previous_usage = NetworkUsage::new(&networks);

    loop {
        sleep(tokio::time::Duration::from_secs(30)).await;

        networks.refresh(true);
        let current_usage = NetworkUsage::new(&networks);
        let unused_bandwidth = current_usage.get_unused_bandwidth(&previous_usage);

        if unused_bandwidth > config.lock().unwrap().threshold {
            let threshold = config.lock().unwrap().threshold;
            let earned_points = ((unused_bandwidth - threshold) as f64 / 1.5).floor() as i64; // Преобразуем в i64
            let earned_points = earned_points.min(10);

            client
                .execute(
                    "UPDATE global_stats SET total_points = total_points + $1",
                    &[&earned_points],
                )
                .await
                .expect("Failed to update total_points");

            println!(
                "Unused bandwidth: {}, Threshold: {}, Earned points: {}",
                unused_bandwidth, threshold, earned_points
            );
        } else {
            println!(
                "Unused bandwidth: {}, Threshold: {}, Not enough traffic to earn points.",
                unused_bandwidth, config.lock().unwrap().threshold
            );
        }

        previous_usage = current_usage;
    }
}

// Шаг 7: Главная страница (HTML)
async fn index() -> impl Responder {
    HttpResponse::Ok().body(include_str!("index.html"))
}

// Шаг 8: Получение статистики через API
async fn get_stats(client: web::Data<Arc<tokio_postgres::Client>>, config: web::Data<Arc<Mutex<NodeConfig>>>) -> impl Responder {
    let threshold = config.lock().unwrap().threshold;

    let row = client
        .query_one("SELECT total_points FROM global_stats LIMIT 1", &[])
        .await
        .expect("Failed to fetch total_points");
    let total_points: i64 = row.get(0);

    HttpResponse::Ok().json(serde_json::json!({
        "unused_bandwidth": 3072, // Примерное значение, можно заменить на реальное
        "threshold": threshold,
        "earned_points": total_points / 2, // Примерное значение
        "total_points": total_points
    }))
}

// Шаг 9: Основной код приложения
#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let port = env::var("PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse::<u16>()
        .expect("PORT must be a number");

    let client = Arc::new(connect_to_db().await.expect("Failed to connect to the database"));

    // Добавляем тестового пользователя (если нужно)
    add_user(&client, "testuser", 0)
        .await
        .expect("Failed to add user");

    let cli = Cli::parse();
    let config = Arc::new(Mutex::new(NodeConfig { threshold: cli.threshold }));

    let db_client_clone = Arc::clone(&client);
    tokio::spawn(monitor_network(db_client_clone, config.clone()));

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(Arc::clone(&client)))
            .app_data(web::Data::new(config.clone()))
            .route("/", web::get().to(index))
            .route("/stats", web::get().to(get_stats))
            .service(
                actix_files::Files::new("/static", "./static").show_files_listing(),
            )
    })
    .bind(("0.0.0.0", port))?
    .run()
    .await
}