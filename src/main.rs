extern crate fst;
extern crate ordermap;
extern crate rand;
extern crate regex;
extern crate reqwest;
extern crate serde;
extern crate serde_json;
extern crate serenity;
extern crate typemap;

use ordermap::OrderMap;
use rand::Rng;
use regex::Regex;
use serde_json::Value as Json;
use serenity::Client;
use serenity::model::gateway::Ready;
use serenity::model::channel::Message;
use serenity::prelude::{ Context, EventHandler };
use std::{ env, fmt };
use std::error::Error;
use std::option::Option;
use std::time::{ Duration, SystemTime, UNIX_EPOCH };

struct Config {
    url: String,
    token: String,
    delay: Duration,
}

const DEFAULT_REQ_DELAY: u64 = 1000 * 60 * 30;
const HELP_TEXT: &str = "__Introducing... **ArrayButt!**__
A revolution in philosophy!
Invoke me with `[]says [date|query]`";
const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June",
    "July", "August", "September", "October", "November", "December"];

struct QuoteYear {
    months: OrderMap<String, QuoteMonth>,
}

struct QuoteMonth {
    quotes: Vec<Quote>,
}

struct Quote {
    year: String,
    month: String,
    text: String,
}

fn parse_quotes(years_dto: Json) -> (OrderMap<String, QuoteYear>, usize) {
    if let Json::Object(years_map) = years_dto {
        let mut years: OrderMap<String, QuoteYear> = OrderMap::new();
        let mut quote_count: usize = 0;
        for (year_key, months_dto) in years_map {
            if let Json::Object(months_map) = months_dto {
                let mut months: OrderMap<String, QuoteMonth> = OrderMap::new();
                for (month_key, quotes_dto) in months_map {
                    if let Json::Array(quotes_vec) = quotes_dto {
                        let mut quotes: Vec<Quote> = Vec::with_capacity(quotes_vec.len());
                        quote_count += quotes_vec.len();
                        for quote_dto in quotes_vec {
                            if let Json::String(quote) = quote_dto {
                                quotes.push(Quote {
                                    year: year_key.clone(),
                                    month: month_key.clone(),
                                    text: quote,
                                });
                            }
                        }
                        months.insert(month_key, QuoteMonth { quotes });
                    }
                }
                years.insert(year_key, QuoteYear { months });
            }
        }
        return (years, quote_count);
    }
    panic!("Parsing error!");
}

#[derive(Debug)]
struct CacheError;

impl Error for CacheError {
    fn description(&self) -> &str {
        "Cache miss!"
    }
}

impl fmt::Display for CacheError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Cache miss!")
    }
}

#[derive(Debug)]
struct CacheRetrievalError(String);

impl Error for CacheRetrievalError {
    fn description(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CacheRetrievalError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Cache retrieval failed: {}", &self.0)
    }
}

impl From<reqwest::Error> for CacheRetrievalError {
    fn from(err: reqwest::Error) -> Self {
        CacheRetrievalError(err.description().to_string())
    }
}

impl From<serde_json::Error> for CacheRetrievalError {
    fn from(err: serde_json::Error) -> Self {
        CacheRetrievalError(err.description().to_string())
    }
}

struct Prefix(Regex);

impl typemap::Key for Prefix {
    type Value = Prefix;
}

struct QuoteCache {
    last_request_time: SystemTime,
    cache: Option<OrderMap<String, QuoteYear>>,
    cache_size: usize,
    request_url: String,
    delay: Duration,
}

impl QuoteCache {
    fn perform_request(&mut self, request_url: reqwest::Url) -> Result<Json, CacheRetrievalError> {
        let json: Json = serde_json::from_str(&reqwest::get(request_url)?.text()?)?;
        Result::Ok(json)
    }
    fn get_quotes(&mut self) -> Result<&OrderMap<String, QuoteYear>, CacheError> {
        let now = SystemTime::now();
        if let Result::Ok(dur) = now.duration_since(self.last_request_time) {
            if &dur >= &self.delay {
                println!("Cache expired! Retrieving...");
                self.last_request_time = now;
                let request_url = reqwest::Url::parse(&self.request_url)
                    .expect("Could not parse request URL!");
                match self.perform_request(request_url) {
                    Result::Ok(json) => {
                        let (cache, cache_size) = parse_quotes(json);
                        self.cache = Option::Some(cache);
                        self.cache_size = cache_size
                    },
                    Result::Err(err) => eprintln!("Cache retrieval failed: {}", err),
                }
            }
        }
        if let Option::Some(ref contents) = self.cache {
            Result::Ok(contents)
        } else {
            Result::Err(CacheError)
        }
    }
}

impl typemap::Key for QuoteCache {
    type Value = QuoteCache;
}

fn send_quote(msg: &Message, quote: &Quote) {
    let month = MONTHS[quote.month.parse::<usize>().unwrap() - 1];
    if let Result::Err(err) = msg.channel_id.send_message(|m| m
        .embed(|e| e
            .description(&quote.text)
            .colour(0x2196F3)
            .footer(|f| f
                .text(format!("Arraying, {} {}", month, quote.year))
                .icon_url("https://avatars1.githubusercontent.com/u/16021050?s=460&v=4")
            )
        )
    ) {
        eprintln!("Failed to send message: {}", err);
    }
}

fn choose_map_entry<V>(map: &OrderMap<String, V>) -> &V {
    map.get_index(rand::thread_rng().gen_range::<usize>(0, map.len())).unwrap().1
}

fn do_command(cache: &mut QuoteCache, msg: &Message, args: &Option<String>) {
    let cache_size = cache.cache_size;
    if let Result::Ok(quotes) = cache.get_quotes() {
        if let &Option::Some(ref query) = args {
            if !query.is_empty() {
                // TODO Implement
                return;
            }
        }
        let mut quotes_flat: Vec<Box<&Quote>> = Vec::with_capacity(cache_size);
        for (_, year) in quotes {
            for (_, month) in &year.months {
                for quote in &month.quotes {
                    &quotes_flat.push(Box::new(quote));
                }
            }
        }
        let quote = rand::thread_rng().choose(&quotes_flat);
        send_quote(&msg, &quote.unwrap());
    } else {
        panic!("Cache was null at command!");
    }
}

struct Handler(Config);

impl EventHandler for Handler {
    fn message(&self, ctx: Context, msg: Message) {
        if !msg.author.bot {
            let mut data = ctx.data.lock();
            if let Option::Some(groups) = data.get::<Prefix>().unwrap().0.captures(&msg.content) {
                do_command(data.get_mut::<QuoteCache>().unwrap(), &msg,
                           &groups.get(1).map(|m| m.as_str().to_string()));
            } else if msg.is_private() {
                if let Result::Err(err) = msg.channel_id.send_message(|m| m.content(HELP_TEXT)) {
                    eprintln!("Failed to send message: {}", err);
                }
            }
        }
    }
    fn ready(&self, ctx: Context, ready: Ready) {
        println!("Authenticated successfully!");

        println!("Building prefix pattern...");
        let prefix_pattern = format!(r"(?:\[]says|<@!?{}>)\s*(?:(.*)\s*)?", ready.user.id);
        println!("Pattern built: {}", prefix_pattern);
        let mut data = ctx.data.lock();
        data.insert::<Prefix>(Prefix(Regex::new(&prefix_pattern).unwrap()));

        println!("Preparing quote cache...");
        let mut cache = QuoteCache {
            last_request_time: UNIX_EPOCH,
            cache: Option::None,
            cache_size: 0,
            request_url: self.0.url.clone(),
            delay: self.0.delay,
        };
        if cache.get_quotes().is_err() {
            panic!("Initial cache population failed!");
        }
        data.insert::<QuoteCache>(cache);

        println!("Bot initialization completed!");
    }
}

fn main() {
    println!("Loading configuration...");
    let config = Config {
        url: env::var("BOT_URL").expect("config->url"),
        token: env::var("BOT_TOKEN").expect("config->token"),
        delay: Duration::from_millis(
            if let std::result::Result::Ok(res) = env::var("BOT_REQ_DELAY") {
                res.parse::<u64>().unwrap_or(DEFAULT_REQ_DELAY)
            } else { DEFAULT_REQ_DELAY }
        )
    };
    println!("url: {}, token: {}, delay: {}", config.url, config.token, config.delay.as_secs());

    println!("Initializing client...");
    let mut bot = Client::new(&config.token.clone(), Handler(config)).expect("Could not create client");
    if let Result::Err(err) = bot.start() {
        panic!(err);
    }
}
