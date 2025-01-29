# Используем официальный образ Rust
FROM rust:1.72 as builder

# Устанавливаем рабочую директорию
WORKDIR /app

# Копируем файлы проекта
COPY . .

# Собираем проект
RUN cargo build --release

# Создаем финальный образ
FROM debian:buster-slim

# Устанавливаем необходимые зависимости
RUN apt-get update && apt-get install -y libssl-dev && rm -rf /var/lib/apt/lists/*

# Копируем исполняемый файл из builder
COPY --from=builder /app/target/release/network_farming /usr/local/bin/

# Команда запуска
CMD ["network_farming", "--threshold=1024"]