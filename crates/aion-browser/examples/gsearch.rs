//! Verifica las señales de stealth del navegador (lo que ve una web anti-bot).
//! Uso: cargo run -p aion-browser --example gsearch

#[tokio::main]
async fn main() {
    let js = "js:JSON.stringify({\
        webdriver: navigator.webdriver, \
        hardwareConcurrency: navigator.hardwareConcurrency, \
        deviceMemory: navigator.deviceMemory, \
        platform: navigator.platform, \
        languages: navigator.languages, \
        plugins: navigator.plugins.length, \
        chrome: !!window.chrome\
    })";
    match aion_browser::debug_html("https://example.com", js).await {
        Ok(s) => {
            let i = s.find('{').unwrap_or(0);
            println!("Señales de stealth vistas por la web:\n{}", &s[i..]);
        }
        Err(e) => println!("❌ {e}"),
    }
}
