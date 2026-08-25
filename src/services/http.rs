use std::sync::OnceLock;
use std::time::Duration;

// reqwest has NO default timeout — a bare reqwest::get() against a stalled host
// hangs forever, and with only 5 db connections a few of those starve the server
const CF_TIMEOUT: Duration = Duration::from_secs(10);

// vjudge returns full standings json, so give it a bit more room
const VJUDGE_TIMEOUT: Duration = Duration::from_secs(15);

// how long to wait on the tcp connect itself
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

// vjudge blocks requests without a proper user-agent (403)
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) SUST-CP-Geeks/1.0";

static CF_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
static VJUDGE_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

fn build(timeout: Duration) -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(timeout)
        .connect_timeout(CONNECT_TIMEOUT)
        .user_agent(USER_AGENT)
        .build()
        // only fails if the tls backend won't initialise, which we can't recover from
        .expect("failed to build http client")
}

// one client per process — a Client owns its connection pool, so building one
// per request throws away keep-alive and churns sockets under load
pub fn codeforces() -> &'static reqwest::Client {
    CF_CLIENT.get_or_init(|| build(CF_TIMEOUT))
}

pub fn vjudge() -> &'static reqwest::Client {
    VJUDGE_CLIENT.get_or_init(|| build(VJUDGE_TIMEOUT))
}
