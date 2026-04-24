# Rust Embedded desde Cero

## paso-08-schedule

[![ESP32 CI](https://github.com/FMFigueroa/paso-08-schedule/actions/workflows/rust_ci.yml/badge.svg)](https://github.com/FMFigueroa/paso-08-schedule/actions/workflows/rust_ci.yml)

<p align="center">
  <img src="docs/rust-board.png" alt="ESP32-C3-DevKit-RUST-1" width="600">
</p>

Scheduler horario con interpolación lineal. El backend le manda al device una curva `{hour, minute, intensity, temperature}` de n puntos, y el modo Auto interpola entre puntos según la hora local (obtenida del SNTP de paso-07). El dominio es circular 24h — el intervalo entre el último y el primer punto atraviesa medianoche.

## Ejemplo de schedule

```json
{
  "type": "SyncSchedule",
  "points": [
    {"hour": 6,  "minute": 0, "intensity": 40, "temperature": 30},
    {"hour": 12, "minute": 0, "intensity": 90, "temperature": 60},
    {"hour": 18, "minute": 30, "intensity": 70, "temperature": 70},
    {"hour": 22, "minute": 0, "intensity": 15, "temperature": 80}
  ]
}
```

A las 09:00, el scheduler interpola linealmente entre (6:00 → 40) y (12:00 → 90): intensity = 40 + (90-40) × 3/6 = 65.

## Fallback

Si el reloj no sincronizó o no hay schedule cargado, el modo Auto cae al ciclo de respiración del paso-03 (`[0, 25, 50, 75, 100, 75, 50, 25]`) para que el LED no quede apagado indefinidamente.

## Probar desde tu terminal

```bash
websocat wss://ws.postman-echo.com/raw
{"type":"SyncSchedule","points":[{"hour":6,"minute":0,"intensity":40,"temperature":30},{"hour":12,"minute":0,"intensity":90,"temperature":60},{"hour":22,"minute":0,"intensity":15,"temperature":80}]}
```

## Roadmap

> Este repo es el **Paso 8** del curso **Rust Embedded desde Cero**.

- [Paso 1 — Scaffold del proyecto](https://github.com/FMFigueroa/paso-01-scaffold)
- [Paso 2 — WiFi Station](https://github.com/FMFigueroa/paso-02-wifi-station)
- [Paso 3 — LED PWM](https://github.com/FMFigueroa/paso-03-led-pwm)
- [Paso 4 — WebSocket Client](https://github.com/FMFigueroa/paso-04-websocket)
- [Paso 5 — Light State Management](https://github.com/FMFigueroa/paso-05-light-state)
- [Paso 6 — Telemetria](https://github.com/FMFigueroa/paso-06-telemetry)
- [Paso 7 — Time Sync (SNTP)](https://github.com/FMFigueroa/paso-07-time-sync)
- **[Paso 8 — Schedule & Auto Mode](https://github.com/FMFigueroa/paso-08-schedule)** ← _este repo_
- [Paso 9 — Concurrencia & Watchdog](https://github.com/FMFigueroa/paso-09-watchdog)


## Documentacion

<a href="https://discord.gg/dYrqe9HZfz"><strong>Unirse al servidor — Curso Rust Embedded</strong></a>

## Licencia

[MIT](LICENSE)
