# 02. 프로젝트 구조 및 컴포넌트 분석

이 문서는 `trading-bot` 프로젝트의 전체적인 폴더 구조와 각 컴포넌트의 책임을 상세히 분석합니다.

## 📂 프로젝트 구조 (Tree)

```text
trading-bot/
├── docs/                   # 프로젝트 문서 세트
├── src/
│   ├── bin/                # 실행 가능한 바이너리 엔트리 포인트
│   │   ├── collector.rs    # 데이터 수집기 실행 파일
│   │   └── backtest.rs     # 백테스터 실행 파일
│   ├── collector/          # 데이터 수집 로직 (App 구동, 태스크 관리)
│   ├── backtest/           # 백테스트 엔진 및 전략 구현체
│   │   └── strategies/     # 개별 트레이딩 전략 정의
│   ├── market_data/        # 외부 거래소 API 연동 (Binance, Alternative.me)
│   ├── order/              # 주문 실행 및 포트폴리오 관리 (Live/Backtest 공통)
│   ├── storage/            # 데이터 저장소 연동 (QuestDB, PostgreSQL)
│   ├── config/             # 애플리케이션 설정 로직
│   ├── types.rs            # 공통 데이터 구조체 정의
│   ├── lib.rs              # 라이브러리 루트 및 모듈 선언
│   └── init.rs             # 로깅 및 초기화 유틸리티
├── migrations/             # SQLx 데이터베이스 마이그레이션 파일
├── Cargo.toml              # 프로젝트 의존성 및 메타데이터
└── docker-compose.yml      # QuestDB, PostgreSQL 등 인프라 설정
```

## 🏗️ 계층별 책임 상세 분석

### 1. 엔트리 포인트 (Binary Layer)
- **`src/bin/collector.rs`**: 수집기 앱을 구동합니다. 환경 변수 로드 및 비동기 런타임(`tokio`)을 시작합니다.
- **`src/bin/backtest.rs`**: CLI 인자(전략명, 기간, 심볼 등)를 파싱하여 백테스트 엔진을 구동하고 결과를 리포트로 저장합니다.

### 2. 애플리케이션 로직 (Application Layer)
- **`src/collector/app.rs`**: 전체 수집 프로세스의 조율자입니다. 설정 로드, DB 라이터 실행, 각 거래소별 수집 태스크(Binance, Alt FNG)를 스폰(Spawn)하고 수명을 관리합니다.
- **`src/backtest/engine.rs`**: 백테스트 시뮬레이션의 핵심 루프를 담당합니다. 데이터를 가져와 이벤트를 발생시키고 전략 핸들러를 호출합니다.

### 3. 서비스 및 엔진 계층 (Service Layer)
- **`src/market_data/`**: 거래소별 복잡한 API 프로토콜을 추상화합니다. WebSocket 재연결, Ping-Pong 처리, REST API 호출 등을 담당합니다.
- **`src/order/`**: 주문 실행을 추상화합니다.
    - `BacktestOrderExecutor`: 메모리 상에서 가상의 자산과 포지션을 계산하고 체결을 시뮬레이션합니다.
    - `LiveOrderExecutor`: 바이낸스 선물 API 및 WebSocket(Order/User Stream)을 활용하여 실제 주문을 집행하고 체결 상태를 실시간으로 관리합니다.

### 4. 데이터 접근 계층 (Persistence Layer)
- **`src/storage/questdb/`**: 고성능 시계열 데이터 저장을 담당합니다. `mpsc` 채널로 들어오는 이벤트를 버퍼링하여 QuestDB에 효율적으로 Flush합니다.
- **`src/storage/postgres/config.rs`**: PostgreSQL을 사용하여 봇의 설정 정보(구독할 심볼 리스트 등)를 관리합니다.
- **`src/storage/postgres/order.rs`**: PostgreSQL 기반 주문/체결 저장소를 담당합니다.

### 5. 도메인 모델 및 유틸리티 (Domain & Utils)
- **`src/types.rs`**: 프로젝트 전체에서 사용되는 핵심 데이터 타입을 정의합니다.
- **`src/config/`**: 구조화된 설정 값들을 정의하며, 런타임에 동적으로 변경될 수 있는 설정들을 포함합니다.

---
[메인으로 돌아가기](./README.md) | [03. 데이터 흐름 분석](./03_data_flow.md)
