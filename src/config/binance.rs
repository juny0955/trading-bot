#[derive(Debug, Clone)]
pub struct BinanceConfig {
    pub net: BinanceNet,
    pub symbols: Vec<SymbolConfig>,
    pub streams: Vec<StreamConfig>,
}

#[derive(Debug, Clone)]
pub enum BinanceNet {
    Testnet,
    Mainnet,
}

#[derive(Debug, Clone)]
pub struct SymbolConfig {
    pub symbol: String,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct StreamConfig {
    pub name: String,
    pub suffix: String,
    pub enabled: bool,
}

impl BinanceConfig {
    pub fn ws_url(&self) -> String {
        let base = match self.net {
            BinanceNet::Mainnet => "wss://fstream.binance.com",
            BinanceNet::Testnet => "wss://stream.binancefuture.com",
        };

        let streams: Vec<String> = self
            .symbols
            .iter()
            .filter(|symbol| symbol.enabled)
            .flat_map(|symbol| {
                self.streams
                    .iter()
                    .filter(|stream| stream.enabled)
                    .map(move |stream| {
                        format!("{}@{}", symbol.symbol.to_lowercase(), stream.suffix)
                    })
            })
            .collect();

        format!("{}/stream?streams={}", base, streams.join("/"))
    }
}
