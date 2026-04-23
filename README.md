```
        _   _ ____    _      ___ _   _ _____ _____ _
       | \ | | __ )  / \    |_ _| \ | |_   _| ____| |
       |  \| |  _ \ / _ \    | ||  \| | | | |  _| | |
       | |\  | |_) / ___ \   | || |\  | | | | |___| |___
       |_| \_|____/_/   \_\ |___|_| \_| |_| |_____|_____|

       a real-time NBA analytics platform written in Rock
```

## What this is

A unified NBA intelligence platform that aggregates, normalizes, and projects
player + team performance from every major basketball data source. Built end to
end in the [Rock programming language](https://github.com/low4k/rock).

```
   data sources              normalize layer            consumer surface
   ───────────────           ────────────────           ──────────────────
   nba.com/stats     ─┐                          ┌──   GET /api/players/:id
   basketball-ref     │                          │     GET /api/teams/:id
   statmuse           │     ┌──────────────┐     │     GET /api/predict/:id
   bball-index        ├───▶ │  unified     │ ───▶├──   GET /api/live
   cleaning the glass │     │  schema +    │     │     GET /api/compare
   dunks & threes     │     │  reconciler  │     │     GET /api/search
   pbp stats          │     └──────────────┘     │     WS  /ws/live/:gid
   nba savant        ─┘            │             └──   /        (dashboard)
                                   ▼
                          ┌──────────────────┐
                          │  storage + cache │
                          │  (json snapshots │
                          │   + redis-style  │
                          │   in-memory map) │
                          └──────────────────┘
```

## Run it

```
rock run src/main.rk
```

Then open [http://localhost:7878](http://localhost:7878).

## Layout

```
   src/
   ├── main.rk            entry point — wires everything and starts the server
   ├── config.rk          tunables: ports, cache TTLs, source URLs
   ├── log.rk             tiny structured logger
   ├── db.rk              persistent JSON store + in-memory cache
   ├── http_util.rk       request/response helpers, JSON encoding
   │
   ├── sources/
   │   ├── nba_official.rk
   │   ├── basketball_ref.rk
   │   ├── statmuse.rk
   │   ├── bball_index.rk
   │   ├── cleaning_the_glass.rk
   │   ├── dunks_and_threes.rk
   │   ├── pbp_stats.rk
   │   └── nba_savant.rk
   │
   ├── normalize.rk       schema reconciliation across sources
   ├── glossary.rk        plain-english explanations of every metric
   │
   ├── players.rk         player profile assembler
   ├── teams.rk           team profile + lineup explorer
   ├── live.rk            live-game polling + websocket fanout
   │
   ├── predict.rk         projection engine: rolling avgs, trend lines,
   │                      confidence bands, opponent adjustment
   ├── compare.rk         player vs player + historical comparisons
   ├── search.rk          natural-language query parser
   │
   ├── server.rk          HTTP routing, websocket loop, static files
   └── web/
       ├── index.html     dashboard
       ├── app.js         charts + live updates
       └── style.css      dark / light themes
```

## Why Rock

Because the language has a real stdlib (http, net, json, fs, time, regex,
strs, math), structured concurrency (`spawn` + `await` + channels), pattern
matching, and pipelines — it turns out to be a natural fit for a data-pipeline
+ live-server style workload.

## License

See LICENSE.
