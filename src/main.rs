use actix_web::{web, App, HttpServer, Responder, HttpResponse};
use actix_files; // Для обслуживания статических файлов
use clap::Parser;
use serde::Deserialize;
use sysinfo::{Networks, System}; // Для работы с сетевым трафиком
use rusqlite::{params, Connection, Result}; // Для работы с SQLite
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

// Шаг 4: Мониторинг сетевого трафика и начисление поинтов
async fn monitor_network(config: Arc<Mutex<NodeConfig>>, points: Arc<Mutex<u64>>) {
    let _system = System::new_all(); // Инициализация системы
    let mut networks = Networks::new_with_refreshed_list(); // Создаем объект Networks
    let mut previous_usage = NetworkUsage::new(&networks);

    loop {
        sleep(tokio::time::Duration::from_secs(30)).await; // Проверяем каждые 30 секунд

        networks.refresh(true); // Обновляем данные о сети
        let current_usage = NetworkUsage::new(&networks);
        let unused_bandwidth = current_usage.get_unused_bandwidth(&previous_usage);

        if unused_bandwidth > config.lock().unwrap().threshold {
            let threshold = config.lock().unwrap().threshold;
            let earned_points = ((unused_bandwidth - threshold) as f64 / 1.5).floor() as u64;
            let earned_points = earned_points.min(10); // Ограничиваем максимум 10 поинтов за интервал

            let mut points_value = points.lock().unwrap();
            *points_value += earned_points;

            println!(
                "Unused bandwidth: {}, Threshold: {}, Earned points: {}, Total points: {}",
                unused_bandwidth, threshold, earned_points, points_value
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

// Шаг 5: Главная страница (HTML)
async fn index() -> impl Responder {
    HttpResponse::Ok().body(include_str!("index.html"))
}

// Шаг 6: Получение статистики через API
async fn get_stats(
    points: web::Data<Arc<Mutex<u64>>>,
    config: web::Data<Arc<Mutex<NodeConfig>>>,
) -> impl Responder {
    let points_value = *points.lock().unwrap();
    let threshold = config.lock().unwrap().threshold;

    // Возвращаем статистику в формате JSON
    HttpResponse::Ok().json(serde_json::json!({
        "unused_bandwidth": 3072, // Примерное значение, можно заменить на реальное
        "threshold": threshold,
        "earned_points": points_value / 2, // Примерное значение
        "total_points": points_value
    }))
}

// Шаг 7: Инициализация базы данных SQLite
fn init_db(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY,
            username TEXT NOT NULL UNIQUE,
            points INTEGER NOT NULL
        )",
        [],
    )?;
    Ok(())
}

// Шаг 8: Добавление пользователя в базу данных
fn add_user(conn: &Connection, username: &str, points: u64) -> Result<()> {
    conn.execute(
        "INSERT INTO users (username, points) VALUES (?1, ?2)",
        params![username, points],
    )?;
    Ok(())
}

// Шаг 9: Обновление поинтов пользователя в базе данных
#[allow(dead_code)] // Подавляем предупреждение, если функция не используется
fn update_points(conn: &Connection, username: &str, points: u64) -> Result<()> {
    conn.execute(
        "UPDATE users SET points = ?1 WHERE username = ?2",
        params![points, username],
    )?;
    Ok(())
}

// Шаг 10: Основной код приложения
#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Получаем порт из переменной окружения или используем значение по умолчанию
    let port = env::var("PORT")
        .unwrap_or_else(|_| "8080".to_string()) // Используем 8080, если PORT не задан
        .parse::<u16>()
        .expect("PORT must be a number");

    // Инициализация CLI-аргументов
    let cli = Cli::parse();
    let config = Arc::new(Mutex::new(NodeConfig { threshold: cli.threshold }));
    let points = Arc::new(Mutex::new(0));

    // Инициализация базы данных SQLite
    let conn = Connection::open("users.db").expect("Failed to open database");
    init_db(&conn).expect("Failed to initialize database");

    // Добавляем тестового пользователя (если нужно)
    add_user(&conn, "testuser", 0).expect("Failed to add user");

    // Запускаем мониторинг трафика
    tokio::spawn(monitor_network(config.clone(), points.clone()));

    // Запускаем HTTP-сервер
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(points.clone()))
            .app_data(web::Data::new(config.clone()))
            .route("/", web::get().to(index))
            .route("/stats", web::get().to(get_stats)) // Маршрут для получения статистики
            .service(
                actix_files::Files::new("/static", "./static").show_files_listing() // Статические файлы
            )
    })
    .bind(("0.0.0.0", port))? // Привязываемся к порту из переменной окружения
    .run()
    .await
}