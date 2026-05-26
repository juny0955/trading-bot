# 05. 변경 이력 (Changelog)

## [v0.3.0] - 2026-05-26

### ✨ 핵심 기능 고도화 (Advanced Trading Features)
- **리스크 컨트롤러(RiskController) 도입**: 주문 전 거래소 필터링, 증거금 확인, 포지션 한도 검증 기능을 추가하였습니다. 한도 초과 시 주문 수량을 자동으로 축소하는 지능형 조정 로직이 포함되었습니다.
- **주문 정정 시스템(OrderReconciler)**: 로컬 DB와 거래소 간의 상태 불일치를 주기적으로 감시하고 해결하는 백그라운드 서비스를 구현하였습니다. (TTL 기반 자동 취소, Drift 감지 등)
- **Outbox 패턴 적용**: 주문 전송의 안정성을 위해 `OutboxOrderService`를 도입하여 주문 요청의 영속성을 보장합니다.
- **계정 상태 실시간 관리**: `AccountState` 모델과 리프레셔를 통해 사용 가능 잔고 및 지갑 밸런스를 실시간으로 추적합니다.

### 🔔 알림 및 보고 (Notifications & Reporting)
- **텔레그램 알림(TelegramNotifier)**: 체결 내역, 리스크 경고, 시스템 상태 등을 텔레그램으로 실시간 전송하는 기능을 추가하였습니다.
- **일일 성과 보고(DailyReporter)**: 매일 지정된 시간에 당일의 매매 성과를 요약하여 보고하는 기능을 추가하였습니다.

### 🏗️ 아키텍처 및 인프라 (Architecture & Infrastructure)
- **무잠금 상태 공유**: `arc_swap`을 도입하여 런타임 설정 및 리스크 한도 변경 시 성능 저하 없는 상태 공유를 실현하였습니다.
- **캐시 레이어 추가**: 호가(BookTicker) 및 심볼 필터 데이터를 위한 전 전용 캐시 어댑터를 추가하여 리스크 검증 속도를 최적화하였습니다.
- **부트스트랩 강화**: `AppBootstrap`을 통해 의존성 주입 구조를 체계화하고 초기화 로직을 개선하였습니다.

## [v0.2.2] - 2026-05-20

### 🏗️ 컴포넌트 명칭 표준화 및 리팩토링 (Component Renaming & Refactoring)
- **명명 규칙 통일**: "Feed" 대신 "Provider", "Writer" 대신 "Sink" 등 인터페이스와 구현체의 역할을 명확히 하는 명칭으로 변경하였습니다.
    - `LiveMarketFeed` -> `MarketDataProvider` (Port)
    - `BinanceLiveFeed` -> `BinanceMarketProvider` (Adapter)
    - `Collector` -> `MarketDataCollector` (Application)
    - `Trader` -> `LiveTrader` (Application)
    - `QuestDbWriter` -> `QuestDbSink` (Adapter)
- **파일 구조 정비**: 클래스명과 파일명을 일치시켜 코드 탐색 효율을 높였습니다.
    - `market_stream.rs` -> `binance_market_provider.rs`
    - `collector.rs` -> `market_data_collector.rs`
    - `trader.rs` -> `live_trader.rs`
    - `config_repo.rs` -> `postgres_config_repository.rs` 등
- **부트스트랩 로직 분리**: `src/init.rs`에서 어플리케이션 초기화 로직을 `src/bootstrap.rs`로 분리하여 구조를 개선하였습니다.

## [v0.2.1] - 2026-05-20

### 📝 문서화 개선 (Documentation Overhaul)
- **도메인 가이드 최신화**: `OrderExecutionEvent` 등 누락된 도메인 이벤트를 추가하였습니다.
- **구조도 정교화**: `adapters/` 하위의 `inbound`, `outbound` 물리적 구조를 반영하고 신규 애플리케이션 서비스를 목록에 추가하였습니다.
- **실거래 흐름도 추가**: 전략 분석부터 주문 체결 수신까지의 시퀀스 다이어그램을 추가하여 데이터 흐름을 명확히 하였습니다.

### 🏗️ 실거래 기능 안정화 (Live Trading Stabilization)
- **AccountEventHandler 구현**: 유저 스트림 이벤트를 처리하여 주문 상태를 갱신하고 체결 내역을 DB에 기록하는 로직을 안정화하였습니다.
- **LiveOrderService 고도화**: 실거래 주문 집행 및 상태 관리 기능을 강화하였습니다.

## [v0.2.0] - 2026-05-19

### 🏗️ 아키텍처 개편 (Architectural Changes)
- **헥사고날 아키텍처(Hexagonal Architecture) 도입**: 기존 레이어드 아키텍처에서 Ports & Adapters 구조로 전면 전환되었습니다.
- **모듈 재구성**: `adapters`, `application`, `domain`, `ports` 모듈로 코드를 분리하여 관심사 분리(SoC)를 강화하였습니다.
- **도메인 모델 정비**: `Trade`, `Depth`, `Kline` 등 도메인 엔티티 명칭을 표준화하고 `MarketDataEvent`를 통한 통합 이벤트 처리를 구현하였습니다.

### ✨ 새로운 기능 (Features)
- **트레이더 바이너리 추가**: 실제 전략을 구동하고 거래를 수행하는 `trader` 실행 파일이 추가되었습니다. (`src/bin/trader.rs`)
- **인바운드/아웃바운드 포트 정의**: `LiveMarketFeed`, `MarketDataSink`, `OrderExecutor` 등 외부 연동을 위한 인터페이스(Port)를 정의하였습니다.

### ⚙️ 기타 변경 사항 (Chores)
- **문서 최신화**: 프로젝트의 구조적 변화에 맞춰 모든 가이드 문서(01~04, README)를 업데이트하였습니다.

## [v0.1.0] - 2026-05-17

### ✨ 새로운 기능 (Features)
- **실거래 주문 모듈 추가**: 바이낸스 선물 API를 통한 실제 주문 집행 기능이 추가되었습니다. (`src/order/live.rs`)
- **바이낸스 Order WS 연결**: WebSocket을 통해 주문 요청을 보내고 응답을 처리하는 로직이 구현되었습니다.
- **바이낸스 User WS 연결**: 유저 스트림을 구독하여 주문 체결(Fill) 이벤트를 실시간으로 수신하고 DB에 기록하는 기능이 추가되었습니다.
- **문서화 자동화 가이드 생성**: 프로젝트 분석 및 가이드 문서 세트(01~04)가 최초로 작성되었습니다.

### ⚙️ 기타 변경 사항 (Chores & Styles)
- **Docker 최적화**: Docker 이미지 빌드 및 실행 옵션이 최적화되었습니다. (2026-05-16)
- **로깅 시스템 개선**: `tracing`을 활용하여 더 명확하고 구조화된 로그를 출력하도록 수정되었습니다. (2026-05-16)

---
[메인으로 돌아가기](./README.md)
