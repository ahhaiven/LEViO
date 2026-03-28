// =========================================================
// LEViO — LEVELYN ESPORTS AI (RUST EDITION v2.0)
// Single-file build | All features included
// =========================================================
//
// FEATURES:
//   ✅ Multi-provider AI (Groq, Gemini, OpenRouter)
//   ✅ Intent detection (legal, writing, esports, chat)
//   ✅ Per-user conversation memory
//   ✅ DuckDuckGo internet search (!search)
//   ✅ Full Discord moderation suite
//   ✅ Auto-moderation (spam, bad words)
//   ✅ Welcome / goodbye messages
//   ✅ Server & user info tools
//   ✅ Role management
//   ✅ Channel lock / slowmode
//   ✅ Bulk message purge
//   ✅ Warn system with strike tracking
//   ✅ Bot uptime & status
//   ✅ Long message chunking (2000 char limit safe)
//
// SETUP:
//   1. Copy .env.example → .env and fill in your keys
//   2. cargo run --release
//
// COMMANDS:
//   !ask <prompt>        — Ask LEViO anything
//   !draft <prompt>      — Write a professional draft
//   !contract <prompt>   — Generate a legal contract
//   !email <prompt>      — Write a formal email
//   !search <query>      — DuckDuckGo web search
//   !define <word>       — Dictionary definition
//   !ping                — Latency check
//   !serverinfo          — Guild statistics
//   !userinfo [@user]    — User details
//   !avatar [@user]      — Show user avatar
//   !uptime              — Bot uptime
//   !botinfo             — LEViO stats
//   !rules               — Server rules
//   !ban <@user> [reason]     — Ban a member
//   !kick <@user> [reason]    — Kick a member
//   !mute <@user> [reason]    — Timeout 10 min
//   !unmute <@user>           — Remove timeout
//   !warn <@user> [reason]    — Issue a warning
//   !warns <@user>            — View member warnings
//   !purge <amount>           — Delete messages
//   !slowmode <seconds>       — Set slowmode
//   !lock                     — Lock channel
//   !unlock                   — Unlock channel
//   !role <add|remove> <@user> <@role> — Manage roles
//   !help                     — Full command list
// =========================================================

use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use std::time::Instant;

use serenity::async_trait;
use serenity::framework::standard::macros::{command, group};
use serenity::framework::standard::{Args, CommandResult, StandardFramework};
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
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
- Casual chat → human, slightly playful, gaming references welcome.\n\
- Official work → formal, structured, professional.\n\
- Moderation → firm but fair, no bias.\n\
\n\
GOAL: Help run Levelyn Esports professionally, efficiently, and with style.";

const BAD_WORDS: &[&str] = &[
    "nigger", "nigga", "faggot", "chink", "spic", "kike", "retard",
];

// =========================================================
// SHARED STATE KEYS
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
        info!("📡 Guilds connected: {}", ready.guilds.len());
        ctx.set_activity(Some(serenity::model::gateway::ActivityData::playing(
            "⚔️ Levelyn Esports | !help",
        )));
    }

    async fn message(&self, ctx: Context, msg: Message) {
        if msg.author.bot {
            return;
        }

        // Auto-moderation
        if auto_mod_check(&ctx, &msg).await {
            return;
        }

        // Respond when mentioned
        if msg.mentions_me(&ctx).await.unwrap_or(false) {
            let bot_id = ctx.cache.current_user().id;
            let clean = msg
                .content
                .replace(&format!("<@{}>", bot_id), "")
                .replace(&format!("<@!{}>", bot_id), "")
                .trim()
                .to_string();

            if clean.is_empty() {
                let _ = msg
                    .reply(
                        &ctx.http,
                        "⚔️ Hey! I'm **LEViO**, Levelyn Esports AI.\nUse `!help` to see all commands, or mention me with a question!",
                    )
                    .await;
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
                    let _ = msg
                        .reply(&ctx.http, format!("⚠️ AI error: {}", e))
                        .await;
                }
            }
        }
    }

    async fn guild_member_addition(
        &self,
        ctx: Context,
        new_member: serenity::model::guild::Member,
    ) {
        let guild_id = new_member.guild_id;
        if let Ok(guild) = guild_id.to_partial_guild(&ctx.http).await {
            if let Some(ch) = guild.system_channel_id {
                let _ = ch
                    .say(
                        &ctx.http,
                        format!(
                            "⚔️ **Welcome to Levelyn Esports, {}!**\n\
                            You're officially part of the squad. Read the rules, have fun, and let's get that W!\n\
                            — *LEViO, Levelyn Esports AI* 🎮",
                            new_member.user.name
                        ),
                    )
                    .await;
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
                let _ = ch
                    .say(
                        &ctx.http,
                        format!(
                            "👋 **{}** has left the server. GG and good luck out there!",
                            user.name
                        ),
                    )
                    .await;
            }
        }
    }
}

// =========================================================
// AUTO-MODERATION
// =========================================================

async fn auto_mod_check(ctx: &Context, msg: &Message) -> bool {
    let content_lower = msg.content.to_lowercase();

    // Bad word filter
    for word in BAD_WORDS {
        if content_lower.contains(word) {
            let _ = msg.delete(&ctx.http).await;
            let _ = msg
                .channel_id
                .say(
                    &ctx.http,
                    format!(
                        "🚫 **{}**, that language isn't allowed here. Consider this a warning.",
                        msg.author.name
                    ),
                )
                .await;
            add_warn(ctx, msg.author.id.get(), &format!("Auto-mod: prohibited language ({})", word)).await;
            return true;
        }
    }

    // Mass-mention spam guard
    if msg.mentions.len() > 5 {
        let _ = msg.delete(&ctx.http).await;
        let _ = msg
            .channel_id
            .say(
                &ctx.http,
                format!("🚫 **{}**, mass mentioning is not allowed.", msg.author.name),
            )
            .await;
        return true;
    }

    false
}

// =========================================================
// MEMORY HELPERS
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
    for (user_msg, ai_msg) in history
        .iter()
        .rev()
        .take(5)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        messages.push(json!({"role": "user", "content": user_msg}));
        messages.push(json!({"role": "assistant", "content": ai_msg}));
    }
    messages
}

async fn add_warn(ctx: &Context, user_id: u64, reason: &str) {
    let data = ctx.data.read().await;
    let warns = data.get::<WarnKey>().unwrap().clone();
    drop(data);
    let mut w = warns.write().await;
    w.entry(user_id).or_default().push(reason.to_string());
}

// =========================================================
// AI PROVIDERS
// =========================================================

async fn call_groq(http: &HttpClient, key: &str, messages: &[Value]) -> Option<String> {
    let body = json!({
        "model": "llama-3.1-8b-instant",
        "messages": messages,
        "max_tokens": 2048,
        "temperature": 0.7
    });

    let resp = http
        .post("https://api.groq.com/openai/v1/chat/completions")
        .bearer_auth(key)
        .json(&body)
        .send()
        .await
        .ok()?;

    let data: Value = resp.json().await.ok()?;
    data["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
}

async fn call_gemini(
    http: &HttpClient,
    key: &str,
    prompt: &str,
    history: &[Value],
) -> Option<String> {
    let mut contents: Vec<Value> = vec![];
    for chunk in history.chunks(2) {
        if chunk.len() == 2 {
            contents.push(json!({
                "role": "user",
                "parts": [{"text": chunk[0]["content"].as_str().unwrap_or("")}]
            }));
            contents.push(json!({
                "role": "model",
                "parts": [{"text": chunk[1]["content"].as_str().unwrap_or("")}]
            }));
        }
    }
    contents.push(json!({
        "role": "user",
        "parts": [{"text": prompt}]
    }));

    let body = json!({
        "system_instruction": {"parts": [{"text": LEVELYN_IDENTITY}]},
        "contents": contents
    });

    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-1.5-flash:generateContent?key={}",
        key
    );

    let resp = http.post(&url).json(&body).send().await.ok()?;
    let data: Value = resp.json().await.ok()?;
    data["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .map(|s| s.to_string())
}

async fn call_openrouter(http: &HttpClient, key: &str, messages: &[Value]) -> Option<String> {
    let body = json!({
        "model": "mistralai/mixtral-8x7b-instruct",
        "messages": messages,
        "max_tokens": 2048
    });

    let resp = http
        .post("https://openrouter.ai/api/v1/chat/completions")
        .bearer_auth(key)
        .header("HTTP-Referer", "https://levelynesports.com")
        .header("X-Title", "LEViO Bot")
        .json(&body)
        .send()
        .await
        .ok()?;

    let data: Value = resp.json().await.ok()?;
    data["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
}

// =========================================================
// INTENT DETECTION
// =========================================================

fn detect_intent(prompt: &str) -> &'static str {
    let p = prompt.to_lowercase();

    if ["contract", "agreement", "legal", "policy", "terms", "nda", "clause", "liability"]
        .iter()
        .any(|w| p.contains(w))
    {
        return "legal";
    }

    if ["email", "announcement", "draft", "write", "letter", "memo", "message", "post"]
        .iter()
        .any(|w| p.contains(w))
    {
        return "writing";
    }

    if ["roster", "tournament", "lineup", "player", "team", "match", "schedule", "sponsor", "coach"]
        .iter()
        .any(|w| p.contains(w))
    {
        return "esports";
    }

    "chat"
}

// =========================================================
// MAIN AI GENERATION
// =========================================================

async fn ai_generate(
    ctx: &Context,
    prompt: &str,
    user_id: u64,
    formal: bool,
) -> Result<String, String> {
    let data = ctx.data.read().await;
    let http = data.get::<HttpKey>().unwrap().clone();
    drop(data);

    let history = load_memory_messages(ctx, user_id).await;
    let intent = detect_intent(prompt);
    info!("🧠 Intent: {} | User: {}", intent, user_id);

    let style_suffix = if formal || matches!(intent, "legal" | "writing") {
        "\nRespond formally, professionally, and in a structured manner."
    } else if intent == "esports" {
        "\nRespond with esports expertise. Use team management language."
    } else {
        ""
    };

    let system = format!("{}{}", LEVELYN_IDENTITY, style_suffix);
    let mut messages: Vec<Value> = vec![json!({"role": "system", "content": system})];
    messages.extend_from_slice(&history);
    messages.push(json!({"role": "user", "content": prompt}));

    let groq_key = env::var("GROQ_KEY").unwrap_or_default();
    let gemini_key = env::var("GEMINI_KEY").unwrap_or_default();
    let openrouter_key = env::var("OPENROUTER_KEY").unwrap_or_default();

    if !groq_key.is_empty() {
        info!("🔄 Trying Groq...");
        if let Some(resp) = call_groq(&http, &groq_key, &messages).await {
            return Ok(format_ai_response(&resp, intent));
        }
        warn!("⚠️ Groq failed, trying next...");
    }

    if !gemini_key.is_empty() {
        info!("🔄 Trying Gemini...");
        if let Some(resp) = call_gemini(&http, &gemini_key, prompt, &history).await {
            return Ok(format_ai_response(&resp, intent));
        }
        warn!("⚠️ Gemini failed, trying next...");
    }

    if !openrouter_key.is_empty() {
        info!("🔄 Trying OpenRouter...");
        if let Some(resp) = call_openrouter(&http, &openrouter_key, &messages).await {
            return Ok(format_ai_response(&resp, intent));
        }
        warn!("⚠️ OpenRouter failed.");
    }

    Err("All AI providers are currently unavailable. Please try again later.".to_string())
}

fn format_ai_response(content: &str, intent: &str) -> String {
    match intent {
        "legal" => format!("📄 **Levelyn Esports — Official Document**\n\n{}", content),
        "writing" => format!("✉️ **Professional Draft**\n\n{}", content),
        "esports" => format!("⚔️ **Esports Advisory**\n\n{}", content),
        _ => content.to_string(),
    }
}

// =========================================================
// DUCKDUCKGO SEARCH
// =========================================================

struct SearchResult {
    title: String,
    snippet: String,
    url: String,
}

async fn duckduckgo_search(
    http: &HttpClient,
    query: &str,
) -> Result<Vec<SearchResult>, String> {
    let encoded = urlencoding::encode(query);
    let url = format!("https://html.duckduckgo.com/html/?q={}", encoded);

    let html = http
        .get(&url)
        .header("User-Agent", "Mozilla/5.0 (compatible; LEViO-Bot/2.0)")
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?
        .text()
        .await
        .map_err(|e| format!("Read error: {}", e))?;

    let document = scraper::Html::parse_document(&html);

    let sel_result = scraper::Selector::parse(".result").unwrap();
    let sel_title = scraper::Selector::parse(".result__title").unwrap();
    let sel_snippet = scraper::Selector::parse(".result__snippet").unwrap();
    let sel_url = scraper::Selector::parse(".result__url").unwrap();

    let mut results = vec![];

    for element in document.select(&sel_result).take(5) {
        let title = element
            .select(&sel_title)
            .next()
            .map(|e| e.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        let snippet = element
            .select(&sel_snippet)
            .next()
            .map(|e| e.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        let url = element
            .select(&sel_url)
            .next()
            .map(|e| e.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        if !title.is_empty() {
            results.push(SearchResult { title, snippet, url });
        }
    }

    Ok(results)
}

// =========================================================
// FREE DICTIONARY API
// =========================================================

async fn fetch_definition(http: &HttpClient, word: &str) -> Result<String, String> {
    let url = format!("https://api.dictionaryapi.dev/api/v2/entries/en/{}", word);
    let resp = http
        .get(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Err(format!("No definition found for **{}**.", word));
    }

    let data: Value = resp.json().await.map_err(|e| e.to_string())?;
    let mut output = format!("📖 **{}**\n\n", word);

    if let Some(entries) = data.as_array() {
        for entry in entries.iter().take(1) {
            if let Some(meanings) = entry["meanings"].as_array() {
                for meaning in meanings.iter().take(2) {
                    let part = meaning["partOfSpeech"].as_str().unwrap_or("unknown");
                    output.push_str(&format!("*{}*\n", part));
                    if let Some(defs) = meaning["definitions"].as_array() {
                        for def in defs.iter().take(2) {
                            let d = def["definition"].as_str().unwrap_or("");
                            output.push_str(&format!("• {}\n", d));
                            if let Some(ex) = def["example"].as_str() {
                                output.push_str(&format!("  *\"{}\"*\n", ex));
                            }
                        }
                    }
                    output.push('\n');
                }
            }
        }
    }

    Ok(output)
}

// =========================================================
// UTILITY HELPERS
// =========================================================

async fn send_chunked(ctx: &Context, msg: &Message, content: &str) {
    if content.len() <= MAX_DISCORD_MSG {
        let _ = msg.reply(&ctx.http, content).await;
        return;
    }

    let mut start = 0;
    let bytes = content.as_bytes();
    let mut first = true;

    while start < bytes.len() {
        let mut end = (start + MAX_DISCORD_MSG).min(bytes.len());
        if end < bytes.len() {
            if let Some(pos) = bytes[start..end].iter().rposition(|&b| b == b'\n') {
                end = start + pos + 1;
            }
        }
        let chunk = &content[start..end];
        if first {
            let _ = msg.reply(&ctx.http, chunk).await;
            first = false;
        } else {
            let _ = msg.channel_id.say(&ctx.http, chunk).await;
        }
        start = end;
    }
}

fn format_duration(secs: u64) -> String {
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;
    let s = secs % 60;
    if days > 0 {
        format!("{}d {}h {}m {}s", days, hours, mins, s)
    } else if hours > 0 {
        format!("{}h {}m {}s", hours, mins, s)
    } else if mins > 0 {
        format!("{}m {}s", mins, s)
    } else {
        format!("{}s", s)
    }
}

// =========================================================
// ░░░░░ COMMANDS ░░░░░
// =========================================================

// ── AI ──────────────────────────────────────────────────

#[command]
async fn ask(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let prompt = args.rest().trim().to_string();
    if prompt.is_empty() {
        msg.reply(&ctx.http, "❓ Usage: `!ask <your question>`").await?;
        return Ok(());
    }
    let _ = msg.channel_id.broadcast_typing(&ctx.http).await;
    let user_id = msg.author.id.get();
    match ai_generate(ctx, &prompt, user_id, false).await {
        Ok(reply) => {
            send_chunked(ctx, msg, &reply).await;
            save_memory(ctx, user_id, &prompt, &reply).await;
        }
        Err(e) => { msg.reply(&ctx.http, format!("⚠️ {}", e)).await?; }
    }
    Ok(())
}

#[command]
async fn draft(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let prompt = args.rest().trim().to_string();
    if prompt.is_empty() {
        msg.reply(&ctx.http, "❓ Usage: `!draft <describe what to write>`").await?;
        return Ok(());
    }
    let _ = msg.channel_id.broadcast_typing(&ctx.http).await;
    let user_id = msg.author.id.get();
    let full_prompt = format!("Write a professional draft: {}", prompt);
    match ai_generate(ctx, &full_prompt, user_id, true).await {
        Ok(reply) => {
            send_chunked(ctx, msg, &reply).await;
            save_memory(ctx, user_id, &full_prompt, &reply).await;
        }
        Err(e) => { msg.reply(&ctx.http, format!("⚠️ {}", e)).await?; }
    }
    Ok(())
}

#[command]
async fn contract(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let prompt = args.rest().trim().to_string();
    if prompt.is_empty() {
        msg.reply(&ctx.http, "❓ Usage: `!contract <details of the contract>`").await?;
        return Ok(());
    }
    let _ = msg.channel_id.broadcast_typing(&ctx.http).await;
    let user_id = msg.author.id.get();
    let full_prompt = format!(
        "Generate a formal legal contract for Levelyn Esports. Details: {}. \
        Include clauses for: parties involved, scope, compensation, confidentiality, \
        termination, and governing law. Format it professionally with section headers.",
        prompt
    );
    match ai_generate(ctx, &full_prompt, user_id, true).await {
        Ok(reply) => {
            send_chunked(ctx, msg, &reply).await;
            save_memory(ctx, user_id, &full_prompt, &reply).await;
        }
        Err(e) => { msg.reply(&ctx.http, format!("⚠️ {}", e)).await?; }
    }
    Ok(())
}

#[command]
async fn email(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let prompt = args.rest().trim().to_string();
    if prompt.is_empty() {
        msg.reply(&ctx.http, "❓ Usage: `!email <describe the email purpose>`").await?;
        return Ok(());
    }
    let _ = msg.channel_id.broadcast_typing(&ctx.http).await;
    let user_id = msg.author.id.get();
    let full_prompt = format!(
        "Write a formal professional email on behalf of Levelyn Esports. \
        Purpose: {}. Include: Subject line, greeting, body paragraphs, closing, and signature.",
        prompt
    );
    match ai_generate(ctx, &full_prompt, user_id, true).await {
        Ok(reply) => {
            send_chunked(ctx, msg, &reply).await;
            save_memory(ctx, user_id, &full_prompt, &reply).await;
        }
        Err(e) => { msg.reply(&ctx.http, format!("⚠️ {}", e)).await?; }
    }
    Ok(())
}

// ── SEARCH ──────────────────────────────────────────────

#[command]
async fn search(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let query = args.rest().trim().to_string();
    if query.is_empty() {
        msg.reply(&ctx.http, "🔍 Usage: `!search <your query>`").await?;
        return Ok(());
    }
    let _ = msg.channel_id.broadcast_typing(&ctx.http).await;

    let data = ctx.data.read().await;
    let http_client = data.get::<HttpKey>().unwrap().clone();
    drop(data);

    match duckduckgo_search(&http_client, &query).await {
        Ok(results) if !results.is_empty() => {
            let mut output = format!("🔍 **Results for:** `{}`\n\n", query);
            for (i, r) in results.iter().enumerate() {
                output.push_str(&format!(
                    "**{}. {}**\n{}\n🔗 {}\n\n",
                    i + 1, r.title, r.snippet, r.url
                ));
            }
            send_chunked(ctx, msg, &output).await;
        }
        Ok(_) => { msg.reply(&ctx.http, "🔍 No results found for that query.").await?; }
        Err(e) => { msg.reply(&ctx.http, format!("⚠️ Search error: {}", e)).await?; }
    }
    Ok(())
}

#[command]
async fn define(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let word = args.rest().trim().to_string();
    if word.is_empty() {
        msg.reply(&ctx.http, "📖 Usage: `!define <word>`").await?;
        return Ok(());
    }
    let _ = msg.channel_id.broadcast_typing(&ctx.http).await;

    let data = ctx.data.read().await;
    let http_client = data.get::<HttpKey>().unwrap().clone();
    drop(data);

    match fetch_definition(&http_client, &word).await {
        Ok(def) => { msg.reply(&ctx.http, def).await?; }
        Err(e) => { msg.reply(&ctx.http, format!("⚠️ {}", e)).await?; }
    }
    Ok(())
}

// ── MODERATION ──────────────────────────────────────────

#[command]
#[required_permissions("BAN_MEMBERS")]
#[only_in(guilds)]
async fn ban(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let target = match args.single::<serenity::model::id::UserId>() {
        Ok(id) => id,
        Err(_) => { msg.reply(&ctx.http, "❌ Usage: `!ban <@user> [reason]`").await?; return Ok(()); }
    };
    let reason = args.rest().trim();
    let reason_str = if reason.is_empty() { "No reason provided." } else { reason };
    let guild_id = msg.guild_id.unwrap();

    match guild_id.ban_with_reason(&ctx.http, target, 0, reason_str).await {
        Ok(_) => {
            let user = target.to_user(&ctx.http).await?;
            msg.channel_id.say(&ctx.http, format!(
                "🔨 **{}** has been banned.\n📋 Reason: `{}`\n— *LEViO Moderation*",
                user.name, reason_str
            )).await?;
        }
        Err(e) => { msg.reply(&ctx.http, format!("❌ Failed to ban: {}", e)).await?; }
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
    let reason_str = if reason.is_empty() { "No reason provided." } else { reason };
    let guild_id = msg.guild_id.unwrap();

    match guild_id.kick_with_reason(&ctx.http, target, reason_str).await {
        Ok(_) => {
            let user = target.to_user(&ctx.http).await?;
            msg.channel_id.say(&ctx.http, format!(
                "👢 **{}** has been kicked.\n📋 Reason: `{}`\n— *LEViO Moderation*",
                user.name, reason_str
            )).await?;
        }
        Err(e) => { msg.reply(&ctx.http, format!("❌ Failed to kick: {}", e)).await?; }
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
    let reason_str = if reason.is_empty() { "Muted by moderator." } else { reason };
    let guild_id = msg.guild_id.unwrap();
    let until = chrono::Utc::now() + chrono::Duration::minutes(10);

    match guild_id
        .edit_member(&ctx.http, target_id, |m| {
            m.disable_communication_until_datetime(until.into())
        })
        .await
    {
        Ok(_) => {
            let user = target_id.to_user(&ctx.http).await?;
            msg.channel_id.say(&ctx.http, format!(
                "🔇 **{}** has been muted for 10 minutes.\n📋 Reason: `{}`\n— *LEViO Moderation*",
                user.name, reason_str
            )).await?;
        }
        Err(e) => { msg.reply(&ctx.http, format!("❌ Failed to mute: {}", e)).await?; }
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
    let guild_id = msg.guild_id.unwrap();

    match guild_id
        .edit_member(&ctx.http, target_id, |m| m.enable_communication())
        .await
    {
        Ok(_) => {
            let user = target_id.to_user(&ctx.http).await?;
            msg.channel_id.say(&ctx.http, format!(
                "🔊 **{}** has been unmuted. — *LEViO Moderation*", user.name
            )).await?;
        }
        Err(e) => { msg.reply(&ctx.http, format!("❌ Failed to unmute: {}", e)).await?; }
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
    let reason_str = if reason.is_empty() { "No reason provided.".to_string() } else { reason };
    let user = target_id.to_user(&ctx.http).await?;

    add_warn(ctx, target_id.get(), &reason_str).await;

    let data = ctx.data.read().await;
    let warns_map = data.get::<WarnKey>().unwrap().clone();
    drop(data);
    let w = warns_map.read().await;
    let count = w.get(&target_id.get()).map(|v| v.len()).unwrap_or(0);
    drop(w);

    msg.channel_id.say(&ctx.http, format!(
        "⚠️ **{}** has been warned. (Total: **{}**)\n📋 Reason: `{}`\n— *LEViO Moderation*",
        user.name, count, reason_str
    )).await?;

    // DM the user
    if let Ok(dm) = user.create_dm_channel(&ctx.http).await {
        let _ = dm.say(&ctx.http, format!(
            "⚠️ You received a warning in **Levelyn Esports**.\n📋 Reason: `{}`\nTotal warnings: **{}**",
            reason_str, count
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

    let data = ctx.data.read().await;
    let warns_map = data.get::<WarnKey>().unwrap().clone();
    drop(data);

    let user = target_id.to_user(&ctx.http).await?;
    let w = warns_map.read().await;

    match w.get(&target_id.get()) {
        Some(list) if !list.is_empty() => {
            let mut out = format!("📋 **Warnings for {}** ({})\n\n", user.name, list.len());
            for (i, reason) in list.iter().enumerate() {
                out.push_str(&format!("{}. {}\n", i + 1, reason));
            }
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
        _ => { msg.reply(&ctx.http, "❌ Usage: `!purge <1–100>`").await?; return Ok(()); }
    };

    let messages = msg
        .channel_id
        .messages(&ctx.http, |r| r.before(msg.id).limit(amount))
        .await?;
    let ids: Vec<_> = messages.iter().map(|m| m.id).collect();
    let count = ids.len();
    msg.channel_id.delete_messages(&ctx.http, &ids).await?;
    let _ = msg.delete(&ctx.http).await;

    let notice = msg
        .channel_id
        .say(&ctx.http, format!("🗑️ Deleted **{}** messages. — *LEViO Moderation*", count))
        .await?;

    // Auto-delete notice after 5 seconds
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
        _ => { msg.reply(&ctx.http, "❌ Usage: `!slowmode <0–21600>` (0 = disable)").await?; return Ok(()); }
    };
    msg.channel_id.edit(&ctx.http, |c| c.rate_limit_per_user(secs)).await?;
    if secs == 0 {
        msg.reply(&ctx.http, "✅ Slowmode **disabled**. — *LEViO Moderation*").await?;
    } else {
        msg.reply(&ctx.http, format!("✅ Slowmode set to **{}s**. — *LEViO Moderation*", secs)).await?;
    }
    Ok(())
}

#[command]
#[required_permissions("MANAGE_CHANNELS")]
#[only_in(guilds)]
async fn lock(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    let everyone = serenity::model::id::RoleId::from(msg.guild_id.unwrap().get());
    msg.channel_id
        .edit(&ctx.http, |c| {
            c.permissions(vec![serenity::model::channel::PermissionOverwrite {
                allow: Permissions::empty(),
                deny: Permissions::SEND_MESSAGES,
                kind: serenity::model::channel::PermissionOverwriteType::Role(everyone),
            }])
        })
        .await?;
    msg.channel_id.say(&ctx.http, "🔒 Channel **locked**. — *LEViO Moderation*").await?;
    Ok(())
}

#[command]
#[required_permissions("MANAGE_CHANNELS")]
#[only_in(guilds)]
async fn unlock(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    let everyone = serenity::model::id::RoleId::from(msg.guild_id.unwrap().get());
    msg.channel_id
        .edit(&ctx.http, |c| {
            c.permissions(vec![serenity::model::channel::PermissionOverwrite {
                allow: Permissions::SEND_MESSAGES,
                deny: Permissions::empty(),
                kind: serenity::model::channel::PermissionOverwriteType::Role(everyone),
            }])
        })
        .await?;
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
    let role_id = match args.single::<serenity::model::id::RoleId>() {
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

// ── UTILITY ─────────────────────────────────────────────

#[command]
async fn ping(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    let before = Instant::now();
    let mut reply = msg.reply(&ctx.http, "🏓 Pinging...").await?;
    let ms = before.elapsed().as_millis();
    reply.edit(&ctx.http, |m| m.content(format!("🏓 **Pong!** `{}ms`", ms))).await?;
    Ok(())
}

#[command]
#[only_in(guilds)]
async fn serverinfo(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    let guild_id = msg.guild_id.unwrap();
    let guild = guild_id.to_partial_guild_with_counts(&ctx.http).await?;
    let owner = guild.owner_id.to_user(&ctx.http).await?;
    let members = guild.approximate_member_count.unwrap_or(0);
    let online = guild.approximate_presence_count.unwrap_or(0);
    let channels = guild.channels(&ctx.http).await.map(|c| c.len()).unwrap_or(0);

    msg.reply(&ctx.http, format!(
        "🏰 **{}** — Server Info\n\n\
        👑 Owner: **{}**\n\
        👥 Members: **{}** ({} online)\n\
        📅 Created: `{}`\n\
        🆔 ID: `{}`\n\
        💬 Channels: **{}** | 🎭 Roles: **{}**\n\
        — *LEViO, Levelyn Esports AI*",
        guild.name, owner.name, members, online,
        guild_id.created_at().to_rfc2822(), guild_id, channels, guild.roles.len()
    )).await?;
    Ok(())
}

#[command]
#[only_in(guilds)]
async fn userinfo(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let target = if let Ok(id) = args.single::<serenity::model::id::UserId>() {
        id.to_user(&ctx.http).await?
    } else {
        msg.author.clone()
    };

    let mut info = format!(
        "👤 **{}** — User Info\n\n\
        🆔 ID: `{}`\n\
        🤖 Bot: **{}**\n\
        📅 Created: `{}`\n\
        🖼️ Avatar: {}",
        target.name, target.id,
        if target.bot { "Yes" } else { "No" },
        target.id.created_at().to_rfc2822(),
        target.face()
    );

    if let Some(guild_id) = msg.guild_id {
        if let Ok(member) = guild_id.member(&ctx.http, target.id).await {
            let joined = member.joined_at.map(|t| t.to_rfc2822()).unwrap_or_else(|| "Unknown".into());
            let roles: Vec<String> = member.roles.iter().map(|r| format!("<@&{}>", r)).collect();
            info.push_str(&format!(
                "\n📆 Joined: `{}`\n🎭 Roles: {}",
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
    } else {
        msg.author.clone()
    };
    msg.reply(&ctx.http, format!("🖼️ **{}'s Avatar**\n{}", target.name, target.face())).await?;
    Ok(())
}

#[command]
async fn uptime(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    let data = ctx.data.read().await;
    let start = data.get::<UptimeKey>().unwrap();
    let secs = start.elapsed().as_secs();
    drop(data);
    msg.reply(&ctx.http, format!("⏱️ Online for **{}**.", format_duration(secs))).await?;
    Ok(())
}

#[command]
async fn botinfo(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    let data = ctx.data.read().await;
    let start = data.get::<UptimeKey>().unwrap();
    let uptime_str = format_duration(start.elapsed().as_secs());
    drop(data);
    let bot_id = ctx.cache.current_user().id;

    msg.reply(&ctx.http, format!(
        "⚔️ **LEViO — Levelyn Esports AI**\n\n\
        🦀 Version: `{}`\n\
        🔧 Language: Rust (Serenity)\n\
        ⏱️ Uptime: `{}`\n\
        🆔 Bot ID: `{}`\n\
        🤖 AI Providers: Groq → Gemini → OpenRouter\n\
        🔍 Search: DuckDuckGo\n\
        📖 Dictionary: Free Dictionary API\n\n\
        *Built with ❤️ for Levelyn Esports*",
        BOT_VERSION, uptime_str, bot_id
    )).await?;
    Ok(())
}

#[command]
async fn rules(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    msg.reply(&ctx.http, "\
⚔️ **Levelyn Esports — Server Rules**\n\n\
**1. Respect Everyone**\n\
Harassment, hate speech, and discrimination will not be tolerated.\n\n\
**2. No Spam**\n\
No flooding, mass-mentioning, or repetitive messages.\n\n\
**3. Stay On-Topic**\n\
Use each channel for its intended purpose.\n\n\
**4. No NSFW Content**\n\
This is a professional esports environment.\n\n\
**5. No Unauthorized Self-Promotion**\n\
No advertising servers or services without mod approval.\n\n\
**6. Respect Staff Decisions**\n\
Staff decisions are final. Open a ticket if you disagree.\n\n\
**7. No Cheating Discussion**\n\
Hacks, exploits, and cheat talk are strictly forbidden.\n\n\
**8. English in Main Channels**\n\
Use English in general channels so everyone can participate.\n\n\
*Violation ladder: Warn → Mute → Kick → Ban*\n\
— *LEViO, Levelyn Esports AI* ⚔️").await?;
    Ok(())
}

#[command("help")]
async fn help_levio(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    msg.reply(&ctx.http, "\
⚔️ **LEViO — Command Reference** | Prefix: `!`\n\n\
🧠 **AI**\n\
`!ask <prompt>` — Ask LEViO anything\n\
`!draft <prompt>` — Professional draft\n\
`!contract <details>` — Legal contract generator\n\
`!email <purpose>` — Formal email writer\n\n\
🔍 **Search & Info**\n\
`!search <query>` — DuckDuckGo web search\n\
`!define <word>` — Dictionary definition\n\n\
🛠️ **Utility**\n\
`!ping` — Latency check\n\
`!serverinfo` — Server stats\n\
`!userinfo [@user]` — User profile\n\
`!avatar [@user]` — View avatar\n\
`!uptime` — Bot uptime\n\
`!botinfo` — About LEViO\n\
`!rules` — Server rules\n\n\
🔨 **Moderation** *(requires permissions)*\n\
`!ban <@user> [reason]` — Ban a member\n\
`!kick <@user> [reason]` — Kick a member\n\
`!mute <@user> [reason]` — Timeout 10 minutes\n\
`!unmute <@user>` — Remove timeout\n\
`!warn <@user> [reason]` — Issue a warning\n\
`!warns <@user>` — View warnings\n\
`!purge <1–100>` — Bulk delete messages\n\
`!slowmode <secs>` — Set channel slowmode\n\
`!lock` — Lock the channel\n\
`!unlock` — Unlock the channel\n\
`!role <add|remove> <@user> <@role>` — Manage roles\n\n\
💡 *Tip: Mention me directly too! @LEViO <your question>*\n\
— *LEViO, Levelyn Esports AI* ⚔️").await?;
    Ok(())
}

// =========================================================
// MAIN ENTRY POINT
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

    info!("🚀 Starting LEViO — Levelyn Esports AI v{} (Rust Edition)", BOT_VERSION);

    let token = env::var("DISCORD_TOKEN").expect("❌ DISCORD_TOKEN not set in .env");

    info!("==== API STATUS ====");
    info!("GROQ:       {}", env::var("GROQ_KEY").map(|k| !k.is_empty()).unwrap_or(false));
    info!("GEMINI:     {}", env::var("GEMINI_KEY").map(|k| !k.is_empty()).unwrap_or(false));
    info!("OPENROUTER: {}", env::var("OPENROUTER_KEY").map(|k| !k.is_empty()).unwrap_or(false));
    info!("====================");

    let framework = StandardFramework::new()
        .group(&AICMDS_GROUP)
        .group(&SEARCHCMDS_GROUP)
        .group(&MODCMDS_GROUP)
        .group(&UTILCMDS_GROUP);

    let framework = framework.configure(|c| {
        c.prefix(BOT_PREFIX)
            .case_insensitivity(true)
            .allow_dm(true)
            .ignore_bots(true)
    });

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
