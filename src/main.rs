// =========================================================
// LEViO — LEVELYN ESPORTS AI (RUST EDITION v2.0)
// Single-file build | Serenity 0.12 compatible
// =========================================================
//
// COMMANDS:
//   !ask <prompt>        — Ask LEViO anything
//   !draft <prompt>      — Professional draft
//   !contract <details>  — Legal contract
//   !email <purpose>     — Formal email
//   !search <query>      — DuckDuckGo search
//   !define <word>       — Dictionary definition
//   !ping                — Latency check
//   !serverinfo          — Guild statistics
//   !userinfo [@user]    — User details
//   !avatar [@user]      — User avatar
//   !uptime              — Bot uptime
//   !botinfo             — About LEViO
//   !rules               — Server rules
//   !help                — Command list
//   !ban <@user> [reason]
//   !kick <@user> [reason]
//   !mute <@user> [reason]    (10 min timeout)
//   !unmute <@user>
//   !warn <@user> [reason]
//   !warns <@user>
//   !purge <1-100>
//   !slowmode <0-21600>
//   !lock / !unlock
//   !role <add|remove> <@user> <@role>
// =========================================================

use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use std::time::Instant;

use serenity::async_trait;
use serenity::builder::{EditChannel, EditMember};
use serenity::framework::standard::macros::{command, group};
use serenity::framework::standard::{Args, CommandResult, Configuration, StandardFramework};
use serenity::model::channel::{Message, PermissionOverwrite, PermissionOverwriteType};
use serenity::model::gateway::Ready;
use serenity::model::id::RoleId;
use serenity::model::Permissions;
use serenity::prelude::*;

use reqwest::Client as HttpClient;
use serde_json::{json, Value};
use tokio::sync::RwLock;
use tracing::{error, info, warn};

// =========================================================
// CONSTANTS & PERSONA
// =========================================================

const BOT_VERSION: &str = "2.0.0";
const BOT_PREFIX: &str = "!";
const MAX_HISTORY: usize = 8;
const MAX_DISCORD_MSG: usize = 1990;

const LEVELYN_IDENTITY: &str = "\
You are LEViO, a highly intelligent female AI assistant for Levelyn Esports.\n\
\n\
PERSONALITY:\n\
- Speak like a calm, confident esports CEO.\n\
- Be intelligent, structured, and clear.\n\
- Add light gaming humor occasionally — never childish.\n\
- Be warm, supportive, and slightly protective of the team.\n\
- Never sound robotic. Sound genuinely human.\n\
\n\
CAPABILITIES:\n\
- Write contracts, agreements, policies, and emails.\n\
- Help manage esports operations, rosters, scheduling, and sponsors.\n\
- Answer general knowledge questions with precision.\n\
- Assist with server moderation decisions fairly.\n\
\n\
STYLE:\n\
- Casual chat: human, slightly playful, gaming references welcome.\n\
- Official work: formal, structured, professional.\n\
- Moderation: firm but fair, no bias.\n\
\n\
GOAL: Help run Levelyn Esports professionally, efficiently, and with style.";

const BAD_WORDS: &[&str] = &[
    "nigger", "nigga", "faggot", "chink", "spic", "kike", "retard",
];

// =========================================================
// SHARED STATE
// =========================================================

struct MemoryKey;
impl TypeMapKey for MemoryKey {
    type Value = Arc<RwLock<HashMap<u64, Vec<(String, String)>>>>;
}

struct WarnKey;
impl TypeMapKey for WarnKey {
    type Value = Arc<RwLock<HashMap<u64, Vec<String>>>>;
}

struct UptimeKey;
impl TypeMapKey for UptimeKey {
    type Value = Instant;
}

struct HttpKey;
impl TypeMapKey for HttpKey {
    type Value = Arc<HttpClient>;
}

// =========================================================
// COMMAND GROUPS
// =========================================================

#[group]
#[commands(ask, draft, contract, email)]
struct AiCmds;

#[group]
#[commands(search, define)]
struct SearchCmds;

#[group]
#[commands(ban, kick, mute, unmute, warn_user, purge, slowmode, lock, unlock, role_manage, warns)]
struct ModCmds;

#[group]
#[commands(ping, serverinfo, userinfo, avatar, uptime, botinfo, rules, help_levio)]
struct UtilCmds;

// =========================================================
// EVENT HANDLER
// =========================================================

struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        info!("✅ LEViO online as: {}", ready.user.name);
        info!("📡 Guilds: {}", ready.guilds.len());
        ctx.set_activity(Some(serenity::model::gateway::ActivityData::playing(
            "⚔️ Levelyn Esports | !help",
        )));
    }

    async fn message(&self, ctx: Context, msg: Message) {
        if msg.author.bot {
            return;
        }

        if auto_mod_check(&ctx, &msg).await {
            return;
        }

        if msg.mentions_me(&ctx).await.unwrap_or(false) {
            let bot_id = ctx.cache.current_user().id;
            let clean = msg
                .content
                .replace(&format!("<@{}>", bot_id), "")
                .replace(&format!("<@!{}>", bot_id), "")
                .trim()
                .to_string();

            if clean.is_empty() {
                let _ = msg.reply(&ctx.http,
                    "⚔️ Hey! I'm **LEViO**, Levelyn Esports AI. Use `!help` to see all commands!",
                ).await;
                return;
            }

            let _ = msg.channel_id.broadcast_typing(&ctx.http).await;
            let user_id = msg.author.id.get();

            match ai_generate(&ctx, &clean, user_id, false).await {
                Ok(reply) => {
                    send_chunked(&ctx, &msg, &reply).await;
                    save_memory(&ctx, user_id, &clean, &reply).await;
                }
                Err(e) => {
                    let _ = msg.reply(&ctx.http, format!("⚠️ AI error: {}", e)).await;
                }
            }
        }
    }

    async fn guild_member_addition(&self, ctx: Context, new_member: serenity::model::guild::Member) {
        if let Ok(guild) = new_member.guild_id.to_partial_guild(&ctx.http).await {
            if let Some(ch) = guild.system_channel_id {
                let _ = ch.say(&ctx.http, format!(
                    "⚔️ **Welcome to Levelyn Esports, {}!**\n\
                    You're part of the squad now. Read the rules, have fun, and let's get that W!\n\
                    — *LEViO, Levelyn Esports AI* 🎮",
                    new_member.user.name
                )).await;
            }
        }
    }

    async fn guild_member_removal(
        &self,
        ctx: Context,
        guild_id: serenity::model::id::GuildId,
        user: serenity::model::user::User,
        _member: Option<serenity::model::guild::Member>,
    ) {
        if let Ok(guild) = guild_id.to_partial_guild(&ctx.http).await {
            if let Some(ch) = guild.system_channel_id {
                let _ = ch.say(&ctx.http, format!(
                    "👋 **{}** has left the server. GG and good luck out there!", user.name
                )).await;
            }
        }
    }
}

// =========================================================
// AUTO-MODERATION
// =========================================================

async fn auto_mod_check(ctx: &Context, msg: &Message) -> bool {
    let lower = msg.content.to_lowercase();

    for word in BAD_WORDS {
        if lower.contains(word) {
            let _ = msg.delete(&ctx.http).await;
            let _ = msg.channel_id.say(&ctx.http, format!(
                "🚫 **{}**, that language isn't allowed here. You've been warned.",
                msg.author.name
            )).await;
            add_warn(ctx, msg.author.id.get(), &format!("Auto-mod: prohibited language ({})", word)).await;
            return true;
        }
    }

    if msg.mentions.len() > 5 {
        let _ = msg.delete(&ctx.http).await;
        let _ = msg.channel_id.say(&ctx.http, format!(
            "🚫 **{}**, mass mentioning is not allowed.", msg.author.name
        )).await;
        return true;
    }

    false
}

// =========================================================
// MEMORY
// =========================================================

async fn save_memory(ctx: &Context, user_id: u64, user_msg: &str, ai_msg: &str) {
    let data = ctx.data.read().await;
    let memory = data.get::<MemoryKey>().unwrap().clone();
    drop(data);
    let mut mem = memory.write().await;
    let history = mem.entry(user_id).or_default();
    history.push((user_msg.to_string(), ai_msg.to_string()));
    if history.len() > MAX_HISTORY {
        let drain = history.len() - MAX_HISTORY;
        history.drain(0..drain);
    }
}

async fn load_memory_messages(ctx: &Context, user_id: u64) -> Vec<Value> {
    let data = ctx.data.read().await;
    let memory = data.get::<MemoryKey>().unwrap().clone();
    drop(data);
    let mem = memory.read().await;
    let history = match mem.get(&user_id) {
        Some(h) => h,
        None => return vec![],
    };
    let mut messages = vec![];
    for (u, a) in history.iter().rev().take(5).collect::<Vec<_>>().into_iter().rev() {
        messages.push(json!({"role": "user", "content": u}));
        messages.push(json!({"role": "assistant", "content": a}));
    }
    messages
}

async fn add_warn(ctx: &Context, user_id: u64, reason: &str) {
    let data = ctx.data.read().await;
    let warns = data.get::<WarnKey>().unwrap().clone();
    drop(data);
    warns.write().await.entry(user_id).or_default().push(reason.to_string());
}

// =========================================================
// AI PROVIDERS
// =========================================================

async fn call_groq(http: &HttpClient, key: &str, messages: &[Value]) -> Option<String> {
    let resp = http
        .post("https://api.groq.com/openai/v1/chat/completions")
        .bearer_auth(key)
        .json(&json!({
            "model": "llama-3.1-8b-instant",
            "messages": messages,
            "max_tokens": 2048,
            "temperature": 0.7
        }))
        .send().await.ok()?;
    let data: Value = resp.json().await.ok()?;
    data["choices"][0]["message"]["content"].as_str().map(|s| s.to_string())
}

async fn call_gemini(http: &HttpClient, key: &str, prompt: &str, history: &[Value]) -> Option<String> {
    let mut contents: Vec<Value> = vec![];
    for chunk in history.chunks(2) {
        if chunk.len() == 2 {
            contents.push(json!({"role":"user","parts":[{"text": chunk[0]["content"].as_str().unwrap_or("")}]}));
            contents.push(json!({"role":"model","parts":[{"text": chunk[1]["content"].as_str().unwrap_or("")}]}));
        }
    }
    contents.push(json!({"role":"user","parts":[{"text": prompt}]}));

    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-1.5-flash:generateContent?key={}",
        key
    );
    let resp = http.post(&url).json(&json!({
        "system_instruction": {"parts": [{"text": LEVELYN_IDENTITY}]},
        "contents": contents
    })).send().await.ok()?;
    let data: Value = resp.json().await.ok()?;
    data["candidates"][0]["content"]["parts"][0]["text"].as_str().map(|s| s.to_string())
}

async fn call_openrouter(http: &HttpClient, key: &str, messages: &[Value]) -> Option<String> {
    let resp = http
        .post("https://openrouter.ai/api/v1/chat/completions")
        .bearer_auth(key)
        .header("HTTP-Referer", "https://levelynesports.com")
        .header("X-Title", "LEViO Bot")
        .json(&json!({
            "model": "mistralai/mixtral-8x7b-instruct",
            "messages": messages,
            "max_tokens": 2048
        }))
        .send().await.ok()?;
    let data: Value = resp.json().await.ok()?;
    data["choices"][0]["message"]["content"].as_str().map(|s| s.to_string())
}

// =========================================================
// INTENT + AI GENERATION
// =========================================================

fn detect_intent(prompt: &str) -> &'static str {
    let p = prompt.to_lowercase();
    if ["contract","agreement","legal","policy","terms","nda","clause","liability"].iter().any(|w| p.contains(w)) { return "legal"; }
    if ["email","announcement","draft","write","letter","memo","post"].iter().any(|w| p.contains(w)) { return "writing"; }
    if ["roster","tournament","player","team","match","schedule","sponsor","coach"].iter().any(|w| p.contains(w)) { return "esports"; }
    "chat"
}

async fn ai_generate(ctx: &Context, prompt: &str, user_id: u64, formal: bool) -> Result<String, String> {
    let data = ctx.data.read().await;
    let http = data.get::<HttpKey>().unwrap().clone();
    drop(data);

    let history = load_memory_messages(ctx, user_id).await;
    let intent = detect_intent(prompt);
    info!("🧠 Intent: {} | User: {}", intent, user_id);

    let style = if formal || matches!(intent, "legal" | "writing") {
        "\nRespond formally, professionally, and in a structured manner."
    } else if intent == "esports" {
        "\nRespond with esports expertise. Use team management language."
    } else { "" };

    let system = format!("{}{}", LEVELYN_IDENTITY, style);
    let mut messages = vec![json!({"role":"system","content": system})];
    messages.extend_from_slice(&history);
    messages.push(json!({"role":"user","content": prompt}));

    let groq_key = env::var("GROQ_KEY").unwrap_or_default();
    let gemini_key = env::var("GEMINI_KEY").unwrap_or_default();
    let openrouter_key = env::var("OPENROUTER_KEY").unwrap_or_default();

    if !groq_key.is_empty() {
        info!("🔄 Trying Groq...");
        if let Some(r) = call_groq(&http, &groq_key, &messages).await {
            return Ok(format_response(&r, intent));
        }
        warn!("⚠️ Groq failed");
    }
    if !gemini_key.is_empty() {
        info!("🔄 Trying Gemini...");
        if let Some(r) = call_gemini(&http, &gemini_key, prompt, &history).await {
            return Ok(format_response(&r, intent));
        }
        warn!("⚠️ Gemini failed");
    }
    if !openrouter_key.is_empty() {
        info!("🔄 Trying OpenRouter...");
        if let Some(r) = call_openrouter(&http, &openrouter_key, &messages).await {
            return Ok(format_response(&r, intent));
        }
        warn!("⚠️ OpenRouter failed");
    }

    Err("All AI providers are unavailable. Please try again later.".to_string())
}

fn format_response(content: &str, intent: &str) -> String {
    match intent {
        "legal"   => format!("📄 **Levelyn Esports — Official Document**\n\n{}", content),
        "writing" => format!("✉️ **Professional Draft**\n\n{}", content),
        "esports" => format!("⚔️ **Esports Advisory**\n\n{}", content),
        _         => content.to_string(),
    }
}

// =========================================================
// DUCKDUCKGO SEARCH
// =========================================================

struct SearchResult { title: String, snippet: String, url: String }

async fn duckduckgo_search(http: &HttpClient, query: &str) -> Result<Vec<SearchResult>, String> {
    let encoded = urlencoding::encode(query);
    let html = http
        .get(&format!("https://html.duckduckgo.com/html/?q={}", encoded))
        .header("User-Agent", "Mozilla/5.0 (compatible; LEViO-Bot/2.0)")
        .send().await.map_err(|e| e.to_string())?
        .text().await.map_err(|e| e.to_string())?;

    let doc = scraper::Html::parse_document(&html);
    let sel_result  = scraper::Selector::parse(".result").unwrap();
    let sel_title   = scraper::Selector::parse(".result__title").unwrap();
    let sel_snippet = scraper::Selector::parse(".result__snippet").unwrap();
    let sel_url     = scraper::Selector::parse(".result__url").unwrap();

    let mut results = vec![];
    for el in doc.select(&sel_result).take(5) {
        let title   = el.select(&sel_title).next().map(|e| e.text().collect::<String>().trim().to_string()).unwrap_or_default();
        let snippet = el.select(&sel_snippet).next().map(|e| e.text().collect::<String>().trim().to_string()).unwrap_or_default();
        let url     = el.select(&sel_url).next().map(|e| e.text().collect::<String>().trim().to_string()).unwrap_or_default();
        if !title.is_empty() { results.push(SearchResult { title, snippet, url }); }
    }
    Ok(results)
}

// =========================================================
// DICTIONARY API
// =========================================================

async fn fetch_definition(http: &HttpClient, word: &str) -> Result<String, String> {
    let resp = http.get(&format!("https://api.dictionaryapi.dev/api/v2/entries/en/{}", word))
        .send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() { return Err(format!("No definition found for **{}**.", word)); }
    let data: Value = resp.json().await.map_err(|e| e.to_string())?;
    let mut out = format!("📖 **{}**\n\n", word);
    if let Some(entries) = data.as_array() {
        for entry in entries.iter().take(1) {
            if let Some(meanings) = entry["meanings"].as_array() {
                for meaning in meanings.iter().take(2) {
                    out.push_str(&format!("*{}*\n", meaning["partOfSpeech"].as_str().unwrap_or("unknown")));
                    if let Some(defs) = meaning["definitions"].as_array() {
                        for def in defs.iter().take(2) {
                            out.push_str(&format!("• {}\n", def["definition"].as_str().unwrap_or("")));
                            if let Some(ex) = def["example"].as_str() {
                                out.push_str(&format!("  *\"{}\"*\n", ex));
                            }
                        }
                    }
                    out.push('\n');
                }
            }
        }
    }
    Ok(out)
}

// =========================================================
// HELPERS
// =========================================================

async fn send_chunked(ctx: &Context, msg: &Message, content: &str) {
    if content.len() <= MAX_DISCORD_MSG {
        let _ = msg.reply(&ctx.http, content).await;
        return;
    }
    let bytes = content.as_bytes();
    let mut start = 0;
    let mut first = true;
    while start < bytes.len() {
        let mut end = (start + MAX_DISCORD_MSG).min(bytes.len());
        if end < bytes.len() {
            if let Some(p) = bytes[start..end].iter().rposition(|&b| b == b'\n') {
                end = start + p + 1;
            }
        }
        let chunk = &content[start..end];
        if first { let _ = msg.reply(&ctx.http, chunk).await; first = false; }
        else { let _ = msg.channel_id.say(&ctx.http, chunk).await; }
        start = end;
    }
}

fn fmt_duration(secs: u64) -> String {
    let (d, h, m, s) = (secs/86400, (secs%86400)/3600, (secs%3600)/60, secs%60);
    if d>0 { format!("{}d {}h {}m {}s",d,h,m,s) }
    else if h>0 { format!("{}h {}m {}s",h,m,s) }
    else if m>0 { format!("{}m {}s",m,s) }
    else { format!("{}s",s) }
}

// =========================================================
// AI COMMANDS
// =========================================================

#[command]
async fn ask(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let prompt = args.rest().trim().to_string();
    if prompt.is_empty() { msg.reply(&ctx.http, "❓ Usage: `!ask <question>`").await?; return Ok(()); }
    let _ = msg.channel_id.broadcast_typing(&ctx.http).await;
    let uid = msg.author.id.get();
    match ai_generate(ctx, &prompt, uid, false).await {
        Ok(r) => { send_chunked(ctx, msg, &r).await; save_memory(ctx, uid, &prompt, &r).await; }
        Err(e) => { msg.reply(&ctx.http, format!("⚠️ {}", e)).await?; }
    }
    Ok(())
}

#[command]
async fn draft(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let prompt = args.rest().trim().to_string();
    if prompt.is_empty() { msg.reply(&ctx.http, "❓ Usage: `!draft <what to write>`").await?; return Ok(()); }
    let _ = msg.channel_id.broadcast_typing(&ctx.http).await;
    let uid = msg.author.id.get();
    let full = format!("Write a professional draft: {}", prompt);
    match ai_generate(ctx, &full, uid, true).await {
        Ok(r) => { send_chunked(ctx, msg, &r).await; save_memory(ctx, uid, &full, &r).await; }
        Err(e) => { msg.reply(&ctx.http, format!("⚠️ {}", e)).await?; }
    }
    Ok(())
}

#[command]
async fn contract(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let prompt = args.rest().trim().to_string();
    if prompt.is_empty() { msg.reply(&ctx.http, "❓ Usage: `!contract <details>`").await?; return Ok(()); }
    let _ = msg.channel_id.broadcast_typing(&ctx.http).await;
    let uid = msg.author.id.get();
    let full = format!(
        "Generate a formal legal contract for Levelyn Esports. Details: {}. \
        Include: parties, scope, compensation, confidentiality, termination, governing law.",
        prompt
    );
    match ai_generate(ctx, &full, uid, true).await {
        Ok(r) => { send_chunked(ctx, msg, &r).await; save_memory(ctx, uid, &full, &r).await; }
        Err(e) => { msg.reply(&ctx.http, format!("⚠️ {}", e)).await?; }
    }
    Ok(())
}

#[command]
async fn email(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let prompt = args.rest().trim().to_string();
    if prompt.is_empty() { msg.reply(&ctx.http, "❓ Usage: `!email <purpose>`").await?; return Ok(()); }
    let _ = msg.channel_id.broadcast_typing(&ctx.http).await;
    let uid = msg.author.id.get();
    let full = format!(
        "Write a formal professional email on behalf of Levelyn Esports. \
        Purpose: {}. Include: subject line, greeting, body, closing, signature.",
        prompt
    );
    match ai_generate(ctx, &full, uid, true).await {
        Ok(r) => { send_chunked(ctx, msg, &r).await; save_memory(ctx, uid, &full, &r).await; }
        Err(e) => { msg.reply(&ctx.http, format!("⚠️ {}", e)).await?; }
    }
    Ok(())
}

// =========================================================
// SEARCH COMMANDS
// =========================================================

#[command]
async fn search(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let query = args.rest().trim().to_string();
    if query.is_empty() { msg.reply(&ctx.http, "🔍 Usage: `!search <query>`").await?; return Ok(()); }
    let _ = msg.channel_id.broadcast_typing(&ctx.http).await;
    let data = ctx.data.read().await;
    let http_client = data.get::<HttpKey>().unwrap().clone();
    drop(data);
    match duckduckgo_search(&http_client, &query).await {
        Ok(results) if !results.is_empty() => {
            let mut out = format!("🔍 **Results for:** `{}`\n\n", query);
            for (i, r) in results.iter().enumerate() {
                out.push_str(&format!("**{}. {}**\n{}\n🔗 {}\n\n", i+1, r.title, r.snippet, r.url));
            }
            send_chunked(ctx, msg, &out).await;
        }
        Ok(_) => { msg.reply(&ctx.http, "🔍 No results found.").await?; }
        Err(e) => { msg.reply(&ctx.http, format!("⚠️ Search error: {}", e)).await?; }
    }
    Ok(())
}

#[command]
async fn define(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let word = args.rest().trim().to_string();
    if word.is_empty() { msg.reply(&ctx.http, "📖 Usage: `!define <word>`").await?; return Ok(()); }
    let _ = msg.channel_id.broadcast_typing(&ctx.http).await;
    let data = ctx.data.read().await;
    let http_client = data.get::<HttpKey>().unwrap().clone();
    drop(data);
    match fetch_definition(&http_client, &word).await {
        Ok(def) => { msg.reply(&ctx.http, def).await?; }
        Err(e)  => { msg.reply(&ctx.http, format!("⚠️ {}", e)).await?; }
    }
    Ok(())
}

// =========================================================
// MODERATION COMMANDS
// =========================================================

#[command]
#[required_permissions("BAN_MEMBERS")]
#[only_in(guilds)]
async fn ban(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let target = match args.single::<serenity::model::id::UserId>() {
        Ok(id) => id,
        Err(_) => { msg.reply(&ctx.http, "❌ Usage: `!ban <@user> [reason]`").await?; return Ok(()); }
    };
    let reason = args.rest().trim();
    let rs = if reason.is_empty() { "No reason provided." } else { reason };
    match msg.guild_id.unwrap().ban_with_reason(&ctx.http, target, 0, rs).await {
        Ok(_) => {
            let user = target.to_user(&ctx.http).await?;
            msg.channel_id.say(&ctx.http, format!(
                "🔨 **{}** banned.\n📋 Reason: `{}`\n— *LEViO Moderation*", user.name, rs
            )).await?;
        }
        Err(e) => { msg.reply(&ctx.http, format!("❌ Failed: {}", e)).await?; }
    }
    Ok(())
}

#[command]
#[required_permissions("KICK_MEMBERS")]
#[only_in(guilds)]
async fn kick(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let target = match args.single::<serenity::model::id::UserId>() {
        Ok(id) => id,
        Err(_) => { msg.reply(&ctx.http, "❌ Usage: `!kick <@user> [reason]`").await?; return Ok(()); }
    };
    let reason = args.rest().trim();
    let rs = if reason.is_empty() { "No reason provided." } else { reason };
    match msg.guild_id.unwrap().kick_with_reason(&ctx.http, target, rs).await {
        Ok(_) => {
            let user = target.to_user(&ctx.http).await?;
            msg.channel_id.say(&ctx.http, format!(
                "👢 **{}** kicked.\n📋 Reason: `{}`\n— *LEViO Moderation*", user.name, rs
            )).await?;
        }
        Err(e) => { msg.reply(&ctx.http, format!("❌ Failed: {}", e)).await?; }
    }
    Ok(())
}

#[command]
#[required_permissions("MODERATE_MEMBERS")]
#[only_in(guilds)]
async fn mute(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let target_id = match args.single::<serenity::model::id::UserId>() {
        Ok(id) => id,
        Err(_) => { msg.reply(&ctx.http, "❌ Usage: `!mute <@user> [reason]`").await?; return Ok(()); }
    };
    let reason = args.rest().trim();
    let rs = if reason.is_empty() { "Muted by moderator." } else { reason };
    let until = chrono::Utc::now() + chrono::Duration::minutes(10);
    let builder = EditMember::new().disable_communication_until(until.into());

    match msg.guild_id.unwrap().edit_member(&ctx.http, target_id, builder).await {
        Ok(_) => {
            let user = target_id.to_user(&ctx.http).await?;
            msg.channel_id.say(&ctx.http, format!(
                "🔇 **{}** muted for 10 minutes.\n📋 Reason: `{}`\n— *LEViO Moderation*", user.name, rs
            )).await?;
        }
        Err(e) => { msg.reply(&ctx.http, format!("❌ Failed: {}", e)).await?; }
    }
    Ok(())
}

#[command]
#[required_permissions("MODERATE_MEMBERS")]
#[only_in(guilds)]
async fn unmute(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let target_id = match args.single::<serenity::model::id::UserId>() {
        Ok(id) => id,
        Err(_) => { msg.reply(&ctx.http, "❌ Usage: `!unmute <@user>`").await?; return Ok(()); }
    };
    let builder = EditMember::new().enable_communication();
    match msg.guild_id.unwrap().edit_member(&ctx.http, target_id, builder).await {
        Ok(_) => {
            let user = target_id.to_user(&ctx.http).await?;
            msg.channel_id.say(&ctx.http, format!(
                "🔊 **{}** unmuted. — *LEViO Moderation*", user.name
            )).await?;
        }
        Err(e) => { msg.reply(&ctx.http, format!("❌ Failed: {}", e)).await?; }
    }
    Ok(())
}

#[command("warn")]
#[required_permissions("MANAGE_MESSAGES")]
#[only_in(guilds)]
async fn warn_user(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let target_id = match args.single::<serenity::model::id::UserId>() {
        Ok(id) => id,
        Err(_) => { msg.reply(&ctx.http, "❌ Usage: `!warn <@user> [reason]`").await?; return Ok(()); }
    };
    let reason = args.rest().trim().to_string();
    let rs = if reason.is_empty() { "No reason provided.".to_string() } else { reason };
    let user = target_id.to_user(&ctx.http).await?;
    add_warn(ctx, target_id.get(), &rs).await;

    let data = ctx.data.read().await;
    let warns_map = data.get::<WarnKey>().unwrap().clone();
    drop(data);
    let count = warns_map.read().await.get(&target_id.get()).map(|v| v.len()).unwrap_or(0);

    msg.channel_id.say(&ctx.http, format!(
        "⚠️ **{}** warned. (Total: **{}**)\n📋 Reason: `{}`\n— *LEViO Moderation*",
        user.name, count, rs
    )).await?;

    if let Ok(dm) = user.create_dm_channel(&ctx.http).await {
        let _ = dm.say(&ctx.http, format!(
            "⚠️ You received a warning in **Levelyn Esports**.\n📋 Reason: `{}`\nTotal warnings: **{}**",
            rs, count
        )).await;
    }
    Ok(())
}

#[command]
#[required_permissions("MANAGE_MESSAGES")]
#[only_in(guilds)]
async fn warns(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let target_id = match args.single::<serenity::model::id::UserId>() {
        Ok(id) => id,
        Err(_) => { msg.reply(&ctx.http, "❌ Usage: `!warns <@user>`").await?; return Ok(()); }
    };
    let user = target_id.to_user(&ctx.http).await?;
    let data = ctx.data.read().await;
    let warns_map = data.get::<WarnKey>().unwrap().clone();
    drop(data);
    let w = warns_map.read().await;
    match w.get(&target_id.get()) {
        Some(list) if !list.is_empty() => {
            let mut out = format!("📋 **Warnings for {}** ({})\n\n", user.name, list.len());
            for (i, r) in list.iter().enumerate() { out.push_str(&format!("{}. {}\n", i+1, r)); }
            msg.reply(&ctx.http, out).await?;
        }
        _ => { msg.reply(&ctx.http, format!("✅ **{}** has no warnings.", user.name)).await?; }
    }
    Ok(())
}

#[command]
#[required_permissions("MANAGE_MESSAGES")]
#[only_in(guilds)]
async fn purge(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let amount: u64 = match args.single::<u64>() {
        Ok(n) if n >= 1 && n <= 100 => n,
        _ => { msg.reply(&ctx.http, "❌ Usage: `!purge <1-100>`").await?; return Ok(()); }
    };
    let messages = msg.channel_id.messages(&ctx.http, serenity::builder::GetMessages::new().before(msg.id).limit(amount as u8)).await?;
    let ids: Vec<_> = messages.iter().map(|m| m.id).collect();
    let count = ids.len();
    msg.channel_id.delete_messages(&ctx.http, &ids).await?;
    let _ = msg.delete(&ctx.http).await;
    let notice = msg.channel_id.say(&ctx.http, format!("🗑️ Deleted **{}** messages. — *LEViO Moderation*", count)).await?;
    let http = ctx.http.clone();
    let ch = msg.channel_id;
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        let _ = ch.delete_message(&http, notice.id).await;
    });
    Ok(())
}

#[command]
#[required_permissions("MANAGE_CHANNELS")]
#[only_in(guilds)]
async fn slowmode(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let secs: u64 = match args.single::<u64>() {
        Ok(n) if n <= 21600 => n,
        _ => { msg.reply(&ctx.http, "❌ Usage: `!slowmode <0-21600>` (0 = disable)").await?; return Ok(()); }
    };
    msg.channel_id.edit(&ctx.http, EditChannel::new().rate_limit_per_user(secs as u16)).await?;
    if secs == 0 { msg.reply(&ctx.http, "✅ Slowmode **disabled**. — *LEViO Moderation*").await?; }
    else { msg.reply(&ctx.http, format!("✅ Slowmode set to **{}s**. — *LEViO Moderation*", secs)).await?; }
    Ok(())
}

#[command]
#[required_permissions("MANAGE_CHANNELS")]
#[only_in(guilds)]
async fn lock(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    let everyone_id = RoleId::new(msg.guild_id.unwrap().get());
    let overwrite = PermissionOverwrite {
        allow: Permissions::empty(),
        deny: Permissions::SEND_MESSAGES,
        kind: PermissionOverwriteType::Role(everyone_id),
    };
    msg.channel_id.create_permission(&ctx.http, overwrite).await?;
    msg.channel_id.say(&ctx.http, "🔒 Channel **locked**. — *LEViO Moderation*").await?;
    Ok(())
}

#[command]
#[required_permissions("MANAGE_CHANNELS")]
#[only_in(guilds)]
async fn unlock(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    let everyone_id = RoleId::new(msg.guild_id.unwrap().get());
    let overwrite = PermissionOverwrite {
        allow: Permissions::SEND_MESSAGES,
        deny: Permissions::empty(),
        kind: PermissionOverwriteType::Role(everyone_id),
    };
    msg.channel_id.create_permission(&ctx.http, overwrite).await?;
    msg.channel_id.say(&ctx.http, "🔓 Channel **unlocked**. — *LEViO Moderation*").await?;
    Ok(())
}

#[command("role")]
#[required_permissions("MANAGE_ROLES")]
#[only_in(guilds)]
async fn role_manage(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let action = match args.single::<String>() {
        Ok(a) => a.to_lowercase(),
        Err(_) => { msg.reply(&ctx.http, "❌ Usage: `!role <add|remove> <@user> <@role>`").await?; return Ok(()); }
    };
    let target_id = match args.single::<serenity::model::id::UserId>() {
        Ok(id) => id,
        Err(_) => { msg.reply(&ctx.http, "❌ Usage: `!role <add|remove> <@user> <@role>`").await?; return Ok(()); }
    };
    let role_id = match args.single::<RoleId>() {
        Ok(id) => id,
        Err(_) => { msg.reply(&ctx.http, "❌ Usage: `!role <add|remove> <@user> <@role>`").await?; return Ok(()); }
    };
    let guild_id = msg.guild_id.unwrap();
    let user = target_id.to_user(&ctx.http).await?;
    match action.as_str() {
        "add" => {
            guild_id.member(&ctx.http, target_id).await?.add_role(&ctx.http, role_id).await?;
            msg.reply(&ctx.http, format!("✅ Role added to **{}**.", user.name)).await?;
        }
        "remove" => {
            guild_id.member(&ctx.http, target_id).await?.remove_role(&ctx.http, role_id).await?;
            msg.reply(&ctx.http, format!("✅ Role removed from **{}**.", user.name)).await?;
        }
        _ => { msg.reply(&ctx.http, "❌ Action must be `add` or `remove`.").await?; }
    }
    Ok(())
}

// =========================================================
// UTILITY COMMANDS
// =========================================================

#[command]
async fn ping(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    let before = Instant::now();
    let mut reply = msg.reply(&ctx.http, "🏓 Pinging...").await?;
    let ms = before.elapsed().as_millis();
    reply.edit(&ctx.http, serenity::builder::EditMessage::new().content(format!("🏓 **Pong!** `{}ms`", ms))).await?;
    Ok(())
}

#[command]
#[only_in(guilds)]
async fn serverinfo(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    let guild_id = msg.guild_id.unwrap();
    let guild = guild_id.to_partial_guild_with_counts(&ctx.http).await?;
    let owner = guild.owner_id.to_user(&ctx.http).await?;
    let channels = guild.channels(&ctx.http).await.map(|c| c.len()).unwrap_or(0);
    msg.reply(&ctx.http, format!(
        "🏰 **{}** — Server Info\n\n\
        👑 Owner: **{}**\n\
        👥 Members: **{}** ({} online)\n\
        📅 Created: `{}`\n\
        🆔 ID: `{}`\n\
        💬 Channels: **{}** | 🎭 Roles: **{}**\n\
        — *LEViO*",
        guild.name, owner.name,
        guild.approximate_member_count.unwrap_or(0),
        guild.approximate_presence_count.unwrap_or(0),
        guild_id.created_at().to_rfc2822(), guild_id,
        channels, guild.roles.len()
    )).await?;
    Ok(())
}

#[command]
#[only_in(guilds)]
async fn userinfo(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let target = if let Ok(id) = args.single::<serenity::model::id::UserId>() {
        id.to_user(&ctx.http).await?
    } else { msg.author.clone() };

    let mut info = format!(
        "👤 **{}** — User Info\n\n🆔 ID: `{}`\n🤖 Bot: **{}**\n📅 Created: `{}`\n🖼️ Avatar: {}",
        target.name, target.id,
        if target.bot { "Yes" } else { "No" },
        target.id.created_at().to_rfc2822(),
        target.face()
    );
    if let Some(guild_id) = msg.guild_id {
        if let Ok(member) = guild_id.member(&ctx.http, target.id).await {
            let joined = member.joined_at.map(|t| t.to_rfc2822()).unwrap_or_else(|| "Unknown".into());
            let roles: Vec<String> = member.roles.iter().map(|r| format!("<@&{}>", r)).collect();
            info.push_str(&format!("\n📆 Joined: `{}`\n🎭 Roles: {}",
                joined,
                if roles.is_empty() { "None".into() } else { roles.join(", ") }
            ));
        }
    }
    msg.reply(&ctx.http, info).await?;
    Ok(())
}

#[command]
async fn avatar(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let target = if let Ok(id) = args.single::<serenity::model::id::UserId>() {
        id.to_user(&ctx.http).await?
    } else { msg.author.clone() };
    msg.reply(&ctx.http, format!("🖼️ **{}'s Avatar**\n{}", target.name, target.face())).await?;
    Ok(())
}

#[command]
async fn uptime(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    let data = ctx.data.read().await;
    let secs = data.get::<UptimeKey>().unwrap().elapsed().as_secs();
    drop(data);
    msg.reply(&ctx.http, format!("⏱️ Online for **{}**.", fmt_duration(secs))).await?;
    Ok(())
}

#[command]
async fn botinfo(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    let data = ctx.data.read().await;
    let uptime = fmt_duration(data.get::<UptimeKey>().unwrap().elapsed().as_secs());
    drop(data);
    msg.reply(&ctx.http, format!(
        "⚔️ **LEViO — Levelyn Esports AI**\n\n\
        🦀 Version: `{}`\n🔧 Language: Rust (Serenity 0.12)\n\
        ⏱️ Uptime: `{}`\n🆔 Bot ID: `{}`\n\
        🤖 AI: Groq → Gemini → OpenRouter\n\
        🔍 Search: DuckDuckGo\n\n\
        *Built with ❤️ for Levelyn Esports*",
        BOT_VERSION, uptime, ctx.cache.current_user().id
    )).await?;
    Ok(())
}

#[command]
async fn rules(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    msg.reply(&ctx.http, "\
⚔️ **Levelyn Esports — Server Rules**\n\n\
**1. Respect Everyone** — No harassment, hate speech, or discrimination.\n\
**2. No Spam** — No flooding, mass-mentions, or repetitive messages.\n\
**3. Stay On-Topic** — Use channels for their intended purpose.\n\
**4. No NSFW Content** — Professional environment only.\n\
**5. No Unauthorized Promotion** — No advertising without mod approval.\n\
**6. Respect Staff** — Decisions are final. Open a ticket to appeal.\n\
**7. No Cheating Discussion** — Hacks and exploits are forbidden.\n\
**8. English in Main Channels** — So everyone can participate.\n\n\
*Violation ladder: Warn → Mute → Kick → Ban*\n\
— *LEViO, Levelyn Esports AI* ⚔️").await?;
    Ok(())
}

#[command("help")]
async fn help_levio(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    msg.reply(&ctx.http, "\
⚔️ **LEViO — Command Reference** | Prefix: `!`\n\n\
🧠 **AI**\n\
`!ask` `!draft` `!contract` `!email`\n\n\
🔍 **Search**\n\
`!search <query>` `!define <word>`\n\n\
🛠️ **Utility**\n\
`!ping` `!serverinfo` `!userinfo` `!avatar` `!uptime` `!botinfo` `!rules`\n\n\
🔨 **Moderation** *(requires permissions)*\n\
`!ban` `!kick` `!mute` `!unmute` `!warn` `!warns`\n\
`!purge` `!slowmode` `!lock` `!unlock` `!role`\n\n\
💡 *Mention me too: @LEViO <question>*\n\
— *LEViO, Levelyn Esports AI* ⚔️").await?;
    Ok(())
}

// =========================================================
// MAIN
// =========================================================

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    info!("🚀 Starting LEViO v{} — Rust Edition", BOT_VERSION);

    let token = env::var("DISCORD_TOKEN").expect("❌ DISCORD_TOKEN not set in .env");

    info!("==== API STATUS ====");
    info!("GROQ:       {}", env::var("GROQ_KEY").map(|k| !k.is_empty()).unwrap_or(false));
    info!("GEMINI:     {}", env::var("GEMINI_KEY").map(|k| !k.is_empty()).unwrap_or(false));
    info!("OPENROUTER: {}", env::var("OPENROUTER_KEY").map(|k| !k.is_empty()).unwrap_or(false));
    info!("====================");

    // Serenity 0.12: Configuration is set globally before building the framework
    let config = Configuration::new()
        .prefix(BOT_PREFIX)
        .case_insensitivity(true)
        .allow_dm(true)
        .ignore_bots(true);

    let framework = StandardFramework::new()
        .group(&AICMDS_GROUP)
        .group(&SEARCHCMDS_GROUP)
        .group(&MODCMDS_GROUP)
        .group(&UTILCMDS_GROUP);

    framework.configure(config);

    let intents = GatewayIntents::all();

    let mut client = Client::builder(&token, intents)
        .event_handler(Handler)
        .framework(framework)
        .await
        .expect("❌ Error creating Discord client");

    {
        let mut data = client.data.write().await;
        data.insert::<MemoryKey>(Arc::new(RwLock::new(HashMap::new())));
        data.insert::<WarnKey>(Arc::new(RwLock::new(HashMap::new())));
        data.insert::<UptimeKey>(Instant::now());
        data.insert::<HttpKey>(Arc::new(
            HttpClient::builder()
                .timeout(std::time::Duration::from_secs(20))
                .user_agent("LEViO-Bot/2.0")
                .build()
                .expect("❌ Failed to build HTTP client"),
        ));
    }

    info!("⚔️ LEViO armed and ready. Connecting to Discord...");

    if let Err(e) = client.start().await {
        error!("❌ Fatal client error: {:?}", e);
    }
}
