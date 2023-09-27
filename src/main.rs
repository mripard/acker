#![warn(missing_debug_implementations)]
#![warn(rust_2018_idioms)]
#![deny(clippy::all)]
#![deny(clippy::pedantic)]
#![deny(clippy::cargo)]
#![allow(clippy::manual_let_else)]
#![allow(clippy::multiple_crate_versions)]

use std::{default::Default, io::Read, path::Path, str::FromStr};

use clap::Parser;
use gix_config::{path::interpolate::Context as PathContext, File as GitFile};
use lettre::{message::Mailbox, Address, Message, SendmailTransport, Transport};
use mail_parser::MessageParser;

const MAX_LINES: usize = 5;

#[derive(Parser, Debug)]
#[command(author, version, about)]
#[allow(clippy::struct_excessive_bools)]
struct Args {
    #[arg(short, long)]
    acked: bool,

    #[arg(short = 'n', long = "dry-run")]
    dry: bool,

    #[arg(short, long)]
    reviewed: bool,

    #[arg(short, long)]
    tested: bool,
}

fn get_user_name(cfg: &GitFile<'_>) -> Option<String> {
    cfg.string_by_key("user.name")
        .map(|n| std::str::from_utf8(n.as_ref()).unwrap().to_string())
}

fn get_user_addr(cfg: &GitFile<'_>) -> Option<Address> {
    cfg.string_by_key("user.email")
        .map(|m| Address::from_str(std::str::from_utf8(m.as_ref()).unwrap()).unwrap())
}

fn get_user_mail(cfg: &GitFile<'_>) -> Mailbox {
    let name = get_user_name(cfg);
    let mail = get_user_addr(cfg).unwrap();

    Mailbox::new(name, mail)
}

fn get_mail_transport(cfg: &GitFile<'_>) -> SendmailTransport {
    if let Some(t) = cfg.path_by_key("sendemail.sendmailcmd").map(|p| {
        let interpolate_options = PathContext {
            ..Default::default()
        };

        let path = p.interpolate(interpolate_options).unwrap();

        SendmailTransport::new_with_command(path.as_os_str())
    }) {
        return t;
    };

    if let Some(t) = cfg.string_by_key("sendemail.smtpserver").map(|s| {
        let s_utf8 = std::str::from_utf8(s.as_ref()).unwrap();

        let path = Path::new(s_utf8);
        if path.exists() {
            return SendmailTransport::new_with_command(path.as_os_str());
        }

        todo!();
    }) {
        return t;
    }

    SendmailTransport::new()
}

fn mailbox_from_addr(a: &mail_parser::Addr<'_>) -> Mailbox {
    let name = a.name.clone().map(String::from);

    let addr = a
        .address
        .clone()
        .map(|a| Address::from_str(&a).unwrap())
        .unwrap();

    Mailbox::new(name, addr)
}

fn mailbox_from_address(address: &mail_parser::Address<'_>) -> Vec<Mailbox> {
    address
        .clone()
        .into_list()
        .iter()
        .map(mailbox_from_addr)
        .collect()
}

fn get_mail_from(msg: &mail_parser::Message<'_>) -> Mailbox {
    mailbox_from_address(msg.from().unwrap()).remove(0)
}

fn get_mail_cc_list(cfg: &GitFile<'_>, msg: &mail_parser::Message<'_>) -> Vec<Mailbox> {
    let user = get_user_mail(cfg);
    let author = get_mail_from(msg);
    let mut recipient_cc_list = Vec::new();

    recipient_cc_list.push(user);

    if let Some(t) = msg.to() {
        recipient_cc_list.append(&mut mailbox_from_address(t));
    }

    if let Some(c) = msg.cc() {
        recipient_cc_list.append(&mut mailbox_from_address(c));
    }

    recipient_cc_list.sort();
    recipient_cc_list.dedup();

    recipient_cc_list
        .into_iter()
        .filter(|u| u != &author)
        .collect()
}

fn get_base_reply(msg: &mail_parser::Message<'_>) -> String {
    let author = get_mail_from(msg);
    let date = msg.date().unwrap();

    let body_text = match &msg.text_bodies().next().unwrap().body {
        mail_parser::PartType::Text(t) => t,
        _ => todo!(),
    };

    let mut reply_body = String::new();

    let name = author.name.unwrap_or(author.email.to_string());

    reply_body.push_str(&format!("On {}, {} wrote:\n", date.to_rfc822(), name));

    for (index, line) in body_text.lines().enumerate() {
        if index >= MAX_LINES {
            reply_body.push_str("> \n");
            reply_body.push_str("> [ ... ]\n");
            break;
        }

        if line == "---" {
            break;
        }

        reply_body.push_str(&format!("> {line}\n").to_owned());
    }

    reply_body
}

fn main() {
    let args = Args::parse();

    let cfg = GitFile::from_globals().expect("Couldn't import Git configuration");

    let mut stdin = std::io::stdin().lock();
    let mut buffer = Vec::new();

    stdin.read_to_end(&mut buffer).unwrap();

    let msg = MessageParser::default().parse(&buffer).unwrap();

    let original_author = get_mail_from(&msg);

    let mut reply_text = get_base_reply(&msg);

    reply_text.push('\n');

    let user = get_user_mail(&cfg);
    if args.acked {
        reply_text.push_str(&format!("Acked-by: {user}\n"));
    }

    if args.reviewed {
        reply_text.push_str(&format!("Reviewed-by: {user}\n"));
    }

    if args.tested {
        reply_text.push_str(&format!("Tested-by: {user}\n"));
    }

    reply_text.push_str(&format!(
        "\nThanks!\n{}\n",
        user.name
            .as_ref()
            .map_or(user.email.as_ref(), |n| n.split(' ').next().unwrap())
    ));

    let msg_id = format!("<{}>", msg.message_id().unwrap());
    let mut builder = Message::builder()
        .date_now()
        .from(user)
        .to(original_author)
        .subject(format!("Re: {}", msg.subject().unwrap()))
        .in_reply_to(msg_id.clone())
        .references(msg_id.clone());

    for user in get_mail_cc_list(&cfg, &msg) {
        builder = builder.cc(user);
    }

    let eml = builder.body(reply_text).unwrap();

    if args.dry {
        println!("{}", std::str::from_utf8(&eml.formatted()).unwrap());
    } else {
        get_mail_transport(&cfg).send(&eml).unwrap();
    }
}
