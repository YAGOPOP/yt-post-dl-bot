use anyhow::{bail, Context};
use linkify::{LinkFinder, LinkKind};
use reqwest::{Client, header};
use std::collections::HashSet;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::AsyncWriteExt;
use url::Url;
use teloxide::prelude::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    
    pretty_env_logger::init();
    log::info!("Starting throw dice bot...");

    let bot = Bot::from_env();
    let client = Client::builder().use_native_tls().build()?;

    teloxide::repl(bot, move |bot: Bot, msg: Message| {
        let client = client.clone();

        async move {
            let chat_id = msg.chat.id;

            match msg.text() {
                Some(msg_txt) => {
                    let post_urls = extract_links(msg_txt, sanitize_yt_post_url);
                    for post_url in post_urls {
                        let img_urls = match get_img_links_from_post(&post_url, &client).await {
                            Ok(v) => v,
                            Err(e) => {
                                bot.send_message(chat_id, format!("Ошибка зпроса: {:?}", e)).await?;
                                HashSet::new()
                            }
                        };
                        for img_url in img_urls {
                            bot.send_message(chat_id, img_url).await?;
                        }
                    }
                }
                None => {
                    bot.send_message(
                        chat_id,
                        "Пожалуйста, отправьте ссылку на пост в сообществе YouTube.",
                    )
                    .await?;
                }
            };
            Ok(())
        }
    })
    .await;

    Ok(())
}

fn extract_links(text: &str, sanitize: fn(Url) -> Option<String>) -> HashSet<String> {
    let mut res = HashSet::new();

    let mut finder = LinkFinder::new();
    finder.kinds(&[LinkKind::Url]);

    for link in finder.links(text) {
        let raw = link.as_str();
        if let Ok(l) = Url::parse(raw)
            && let Some(link) = sanitize(l)
        {
            res.insert(link);
        }
    }

    res
}

fn sanitize_yt_post_url(mut url: Url) -> Option<String> {
    let host = url.host_str()?;
    if !is_domain_or_subdomain(host, "youtube.com") {
        return None;
    }
    if url.path_segments()?.next() != Some("post") {
        return None;
    }
    url.set_query(None);

    Some(url.as_str().to_owned())
}

fn sanitize_ggpht_url(url: Url) -> Option<String> {
    if url.host_str()? != "yt3.ggpht.com" {
        return None;
    }
    let str_url = url.as_str();
    let i = str_url.find("=")?;

    Some(format!("{}s0", &str_url[..=i]))
}

fn is_domain_or_subdomain(host: &str, domain: &str) -> bool {
    host == domain
        || host
            .strip_suffix(domain)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

fn figure_out_response_file_extension(hv: &header::HeaderMap) -> anyhow::Result<&'static str> {
    let content_type = hv
        .get(header::CONTENT_TYPE)
        .context("Ошибка: в ответе от сервера на запрос по прямой ссылке картинки нет контента.")?
        .to_str()
        .context("Ошибка: некорректный CONTENT_TYPE в ответе сервера.")?;

    match content_type {
        "image/jpeg" => Ok("jpeg"),
        "image/gif" => Ok("gif"),
        "image/png" => Ok("png"),
        other => bail!("Ошибка: не предусмотренный тип контента в ответе: {other}"),
    }
}

async fn get_img_links_from_post(
    indirect_url: &str,
    client: &Client,
) -> anyhow::Result<HashSet<String>> {
    let resp = client
        .get(indirect_url)
        .send()
        .await
        .with_context(|| format!("Не удалось отправить запрос к {indirect_url}"))?
        .error_for_status()
        .with_context(|| format!("Сервер вернул ошибочный статус для {indirect_url}"))?;

    let resp_text = resp
        .text()
        .await
        .context("Не удалось прочитать тело ответа.")?;

    let img_urls = extract_links(&resp_text, sanitize_ggpht_url);

    Ok(img_urls)
}
