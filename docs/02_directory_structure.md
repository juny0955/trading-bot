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
│   │   ├── backtest_sweep.rs # 파라미터 최적화 스윕 실행 파일
│   │   └── trader.rs       # 실거래 트레이더 실행 파일
│   ├── adapters/           # 외부 시스템과의 연동 (Adapters)
│   │   ├── inbound/        # 입력 어댑터 (External -> Core)
│   │   │   ├── binance/    # 바이낸스 시장/유저 스트림 수신
│   │   │   └── alternative/# Alternative.me (F&G Index) 연동
│   │   └── outbound/       # 출력 어댑터 (Core -> External)
│   │       ├── binance/    # 바이낸스 API (주문, 계정, 정보)
│   │       ├── cache/      # 메모리 내 캐시 (호가, 필터)
│   │       ├── postgres/   # PostgreSQL 영속성 구현
│   │       ├── questdb/    # QuestDB 데이터 저장
│   │       ├── questdb_rest/# QuestDB 데이터 조회
│   │       └── telegram/   # 텔레그램 알림 전송
│   ├── application/        # 유즈케이스 및 서비스 로직 (Application)
│   │   ├── risk_controller.rs # 리스크 사전 검증 및 수량 조정
│   │   ├── order_reconciler.rs # 주문 상태 동기화 및 TTL 관리
│   │   ├── outbox_order_service.rs # Outbox 패턴 기반 주문 서비스
│   │   ├── account_state.rs # 실시간 계좌 상태 관리
│   │   ├── account_event_handler.rs # 체결/상태 변경 이벤트 처리
│   │   ├── daily_reporter.rs # 일일 성과 보고
│   │   ├── live_trader.rs     # 트레이더 메인 루프
│   │   └── backtest/       # 백테스트 엔진 및 포트폴리오 시뮬레이션
│   ├── domain/             # 핵심 도메인 모델 및 비즈니스 규칙 (Domain)
│   │   ├── order.rs        # 주문, 체결, 상태 전이 규칙
│   │   ├── position.rs     # 포지션 및 전략별 상태
│   │   └── strategies/     # 트레이딩 전략 (EMA, MACD, RSI 등)
│   ├── ports/              # 외부와의 상호작용 인터페이스 (Ports)
│   ├── config/             # 애플리케이션 및 런타임 설정
│   ├── lib.rs              # 라이브러리 루트 및 모듈 선언
│   ├── bootstrap.rs        # DI 컨테이너 및 앱 부트스트랩
│   └── init.rs             # 로깅 및 초기 설정 유틸리티
```

## 🏗️ 헥사고날 계층별 책임 상세 분석

### 1. 도메인 계층 (Domain Layer) - Core
- **책임**: 외부 프레임워크에 의존하지 않는 순수한 비즈니스 규칙을 정의합니다.
- **주요 컴포넌트**: `Order`, `Position`, `Strategy` 트레이트, `OrderStatus` 전이 로직.

### 2. 포트 계층 (Ports Layer) - Interfaces
- **책임**: 애플리케이션 코어가 외부와 통신하기 위한 추상 인터페이스(Trait)를 정의합니다.
- **Inbound Ports**: `MarketDataProvider`, `TradingUseCase`.
- **Outbound Ports**: `OrderRepository`, `OrderExecutor`, `NotificationSink`.

### 3. 애플리케이션 계층 (Application Layer) - Usecases
- **책임**: 비즈니스 흐름을 오케스트레이션하고 안정성을 보장합니다.
- **주요 컴포넌트**: 
    - `RiskController`: 주문 전 리스크 검증.
    - `OrderReconciler`: 로컬-거래소 간 상태 불일치 해소.
    - `OutboxOrderService`: 안정적인 주문 제출 보장.
    - `DailyReporter`: 성과 분석 및 알림 발송.

### 4. 어댑터 계층 (Adapters Layer) - Infrastructure
- **책임**: 포트 인터페이스를 구체적으로 구현합니다.
- **주요 컴포넌트**: `BinanceOrderExecutor`, `PostgresOrderRepository`, `TelegramNotifier`.

### 5. 바이너리 계층 (Binary Layer) - Entry Points
- **`src/bin/collector.rs`**: 데이터 수집기를 구동합니다.
- **`src/bin/backtest.rs`**: 과거 데이터를 기반으로 시뮬레이션을 수행합니다.
- **`src/bin/trader.rs`**: 실제 거래 전략을 실행하는 봇을 구동합니다.

---
[메인으로 돌아가기](./README.md) | [03. 데이터 흐름 분석](./03_data_flow.md)
