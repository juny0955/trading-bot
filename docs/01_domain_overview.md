# 01. 도메인 및 비즈니스 로직 개요

이 문서는 `trading-bot` 프로젝트가 해결하고자 하는 비즈니스 문제와 이를 구성하는 핵심 도메인 모델에 대해 설명합니다.

## 🎯 프로젝트 목적
트레이딩 봇의 성공은 **데이터의 정확한 수집**과 **전략의 엄격한 검증**에 달려 있습니다. 이 프로젝트는 다음 두 가지 핵심 목표를 달성하기 위해 설계되었습니다.
1. **무손실 데이터 수집**: 바이낸스 선물 시장의 모든 거래(Trade), 호가(Depth), 캔들(Kline), 펀딩비(Mark Price) 등을 누락 없이 실시간으로 저장합니다.
2. **현실적인 시뮬레이션**: 단순 종가 기반 백테스트를 넘어, 호가창(Order Book) 데이터를 활용하여 슬리피지와 체결 가능성을 고려한 정교한 백테스팅 환경을 제공합니다.

## 🧩 주요 도메인 엔티티 (Entities)

### 1. 시장 데이터 (Market Data)
`src/domain/market_data.rs`에 정의된 핵심 데이터 모델입니다.
- **Trade**: 개별 체결 데이터 (가격, 수량, 체결 시간, 메이커 여부)
- **Depth**: 호가창 데이터 (Bids/Asks 각 단계의 가격 및 잔량)
- **BookTicker**: 최우선 매수/매도 호가 및 수량 (Best Bid/Ask)
- **Kline**: 캔들 데이터 (시가, 고가, 저가, 종가, 거래량 등)
- **MarkPrice**: 시장 평균가 및 펀딩 비율 정보
- **FearGreed**: Alternative.me에서 제공하는 공포/탐욕 지수

### 2. 주문 및 포트폴리오 (Order & Portfolio)
`src/domain/order.rs`, `src/domain/position.rs` 및 `src/application/backtest/portfolio.rs`에서 관리되는 모델입니다.
- **OrderRequest**: 주문 요청 (심볼, 사이드, 타입, 수량, 가격, TimeInForce 등)
- **Order**: 시스템 내의 주문 상태를 나타내는 모델 (ID, 상태, 체결 수량, 평균 체결가 등)
- **Fill**: 실제 체결된 결과 (체결가, 체결 수량, 수수료, 체결 시각)
- **Position**: 현재 보유 중인 포지션 정보 (방향, 수량, 진입가, 진입 시각)
- **PositionLifecycle**: 진입부터 청산까지의 전체 주기를 추적하는 모델 (MFE, MAE, 실현 손익 포함)

### 3. 리스크 및 한도 (Risk & Limits)
`src/application/risk_controller.rs`와 `src/ports/outbound/risk_config_source.rs`에 정의된 모델입니다.
- **RiskLimits**: 계좌 수준의 안전 가드레일 (최대 레버리지, 포지션 한도, 주문 TTL 등)
- **RiskError**: 필터 위반, 증거금 부족, 한도 초과 등 사전 검증 실패 사유를 정의합니다.

### 4. 결정 및 맥락 (Decision & Context)
`src/domain/trade_decision.rs`에 정의된 모델로, 거래의 '이유'를 기록합니다.
- **TradeDecision**: 주문 생성 시점의 시장 데이터, 지표 값, 전략 파라미터 등을 스냅샷으로 저장하여 사후 분석 및 학습 데이터로 활용합니다.

### 5. 계정 및 포지션 상태 (Account & Position State)
실시간 거래를 위해 현재 계좌 잔고와 보유 포지션을 추적하며, 재시작 시에도 상태를 유지하기 위한 런타임 상태를 관리합니다.
- **AccountState**: 사용 가능 잔고 및 지갑 밸런스를 스레드 안전하게 관리합니다.
- **StrategyPosition**: 각 전략 인스턴스별로 심볼별 포지션(수량, 진입가)을 독립적으로 관리합니다.
- **PositionRuntimeState**: `TradeManager`가 포지션을 관리하는 데 필요한 가변 상태(현재 TP 단계, 트레일링 스탑 기준가 등)를 담고 있으며, DB에 영속화되어 시스템 재시작 후에도 연속성 있는 관리를 보장합니다.

### 6. 통합 이벤트 (Events)
시스템 내의 비동기 통신을 위해 다음 두 가지 핵심 이벤트 모델을 사용합니다.
- **MarketDataEvent**: 다양한 시장 데이터를 하나의 열거형(Enum)으로 통합하여 처리 효율성을 높입니다. (`src/domain/event.rs`)
- **OrderExecutionEvent**: 거래소로부터 수신한 주문 상태 변경 및 체결 정보를 담고 있으며, 계좌 상태 동기화의 핵심입니다. (`src/application/order_execution_event.rs`)

## 💡 핵심 비즈니스 로직

### 데이터 수집 (Collection)
- **인바운드 포트(MarketDataProvider)**: 외부 거래소의 실시간 스트림을 구독합니다.
- **아웃바운드 포트(MarketDataSink)**: 수집된 데이터를 QuestDB 등 저장소에 배치(Batch) 방식으로 저장하여 고속 쓰기를 실현합니다.

### 리스크 관리 (Risk Control)
- **사전 검증(Pre-trade Check)**: 주문이 거래소로 전송되기 전, 거래소 필터(최소 수량 등)와 사용자 정의 리스크 한도를 검사합니다.
- **자동 수량 조정**: 포지션 한도를 초과하는 주문의 경우, 거절하는 대신 허용 가능한 최대 수량으로 자동 조정하여 실행합니다.

### 어댑티브 포지션 관리 (Adaptive Management)
- **TradeManager**: 포지션 진입 후, 실시간 가격 움직임과 변동성(ATR)을 바탕으로 트레일링 스탑을 조정하거나 단계별 부분 익절(Partial TP)을 실행합니다.
- **SmartEntry**: 시장가와 지정가 사이에서 체결 비용과 슬리피지를 고려하여 최적의 진입 방식을 결정합니다.

### 주문 정정 및 동기화 (Reconciliation & Outbox)
- **Outbox 패턴**: 주문을 DB에 먼저 기록(`PendingSubmit`)한 후 거래소에 전송하여 시스템 장애 시에도 주문 추적이 가능하게 합니다.
- **Reconciler**: 로컬 데이터베이스와 거래소의 주문 상태 간 차이(Drift)를 주기적으로 감지하고 자동으로 동기화합니다.
- **Orphan 처리**: 로컬에 기록되지 않았으나 거래소에서 발생한 주문(Orphan)을 감지하여 매칭하거나 정리합니다.

### 백테스트 엔진 (Backtest Engine)

- **이벤트 병합(Event Merging)**: 과거의 캔들 데이터와 호가 데이터를 타임스탬프 기준으로 정렬하여 실시간 상황과 유사한 이벤트 스트림을 재구성합니다.
- **전략 인터페이스(Strategy Interface)**: `on_bar`, `on_depth` 등의 핸들러를 통해 전략 로직을 분리하여 구현할 수 있습니다.
- **체결 시뮬레이션**: 호가창(Depth) 기반의 매칭 알고리즘을 사용하여 현실적인 슬리피지와 체결 지연을 시뮬레이션합니다.

---
[메인으로 돌아가기](./README.md) | [02. 프로젝트 구조 분석](./02_directory_structure.md)
