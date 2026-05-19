# 02. 프로젝트 구조 및 컴포넌트 분석

이 문서는 `trading-bot` 프로젝트의 헥사고날 아키텍처(Hexagonal Architecture) 구조와 각 컴포넌트의 책임을 상세히 분석합니다.

## 📂 프로젝트 구조 (Tree)

```text
trading-bot/
├── docs/                   # 프로젝트 문서 세트
├── src/
│   ├── bin/                # 실행 가능한 바이너리 엔트리 포인트
│   │   ├── collector.rs    # 데이터 수집기 실행 파일
│   │   ├── backtest.rs     # 백테스터 실행 파일
│   │   └── trader.rs       # 실거래 트레이더 실행 파일
│   ├── adapters/           # 외부 시스템과의 연동 (Adapters)
│   │   ├── binance/        # 바이낸스 API 및 WebSocket 연동
│   │   ├── alternative/    # Alternative.me (F&G Index) 연동
│   │   ├── questdb/        # QuestDB (ILP) 데이터 저장
│   │   ├── questdb_rest/   # QuestDB (REST) 데이터 조회
│   │   └── postgres/       # PostgreSQL 설정 및 주문 저장
│   ├── application/        # 유즈케이스 및 서비스 로직 (Application)
│   │   ├── collector.rs    # 수집기 오케스트레이션
│   │   ├── trader.rs       # 트레이더 유즈케이스
│   │   └── backtest/       # 백테스트 엔진 및 시뮬레이션
│   ├── domain/             # 핵심 도메인 모델 및 비즈니스 규칙 (Domain)
│   │   ├── market_data.rs  # 시장 데이터 엔티티
│   │   ├── order.rs        # 주문 및 체결 엔티티
│   │   └── strategies/     # 트레이딩 전략 정의
│   ├── ports/              # 외부와의 상호작용 인터페이스 (Ports)
│   │   ├── inbound/        # 입력 포트 (Feed 등)
│   │   └── outbound/       # 출력 포트 (Sink, Repository, Executor 등)
│   ├── config/             # 애플리케이션 설정 로직
│   ├── lib.rs              # 라이브러리 루트 및 모듈 선언
│   └── init.rs             # 로깅 및 초기화 유틸리티
├── migrations/             # SQLx 데이터베이스 마이그레이션 파일
├── Cargo.toml              # 프로젝트 의존성 및 메타데이터
└── docker-compose.yml      # QuestDB, PostgreSQL 등 인프라 설정
```

## 🏗️ 헥사고날 계층별 책임 상세 분석

### 1. 도메인 계층 (Domain Layer) - Core
- **책임**: 외부 프레임워크나 라이브러리에 의존하지 않는 순수한 비즈니스 모델과 규칙을 정의합니다.
- **주요 컴포넌트**: `Trade`, `Order`, `Strategy` 트레이트 등.

### 2. 포트 계층 (Ports Layer) - Interfaces
- **책임**: 애플리케이션 코어가 외부와 통신하기 위한 추상적인 인터페이스를 정의합니다.
- **Inbound Ports**: 외부에서 코어로 들어오는 통로 (`LiveMarketFeed` 등).
- **Outbound Ports**: 코어에서 외부 시스템(DB, 거래소)으로 나가는 통로 (`MarketDataSink`, `OrderExecutor` 등).

### 3. 애플리케이션 계층 (Application Layer) - Usecases
- **책임**: 포트를 통해 들어온 요청을 처리하기 위해 도메인 모델을 조작하고 전체적인 흐름을 제어합니다.
- **주요 컴포넌트**: `Collector`, `BacktestEngine`, `LiveOrderService`.

### 4. 어댑터 계층 (Adapters Layer) - Infrastructure
- **책임**: 포트 인터페이스를 실제로 구현하여 외부 시스템과 직접 통신합니다.
- **주요 컴포넌트**: `BinanceApiAdapter`, `QuestDbWriter`, `PostgresRepository`.

### 5. 바이너리 계층 (Binary Layer) - Entry Points
- **`src/bin/collector.rs`**: 데이터 수집기를 구동합니다.
- **`src/bin/backtest.rs`**: 과거 데이터를 기반으로 시뮬레이션을 수행합니다.
- **`src/bin/trader.rs`**: 실제 거래 전략을 실행하는 봇을 구동합니다.

---
[메인으로 돌아가기](./README.md) | [03. 데이터 흐름 분석](./03_data_flow.md)
