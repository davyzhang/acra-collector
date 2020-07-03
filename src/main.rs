//! ACRA Collector
//! 
//! A simple collector for ACRA that does the following:
//! 
//! - Append report to `crashes.txt` file
//! - Send an e-mail with the stack trace
extern crate env_logger;
extern crate iron;
extern crate lettre;
extern crate router;
extern crate serde;
#[macro_use] extern crate serde_derive;
extern crate serde_json;

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};

use iron::prelude::*;
use iron::{middleware, status};
use lettre::{
    Message,
    Transport,
    transport::smtp::{
        authentication::Credentials,
        client::net::TlsParameters,
        SmtpTransport,
        Tls,
    },
};
use router::Router;
use serde_json::Value;

#[derive(Deserialize, Debug, Clone)]
struct Config {
    host: String,
    port: u16,
    email_from: String,
    email_to: String,
    smtp_host: String,
    smtp_port: u16,
    smtp_user: String,
    smtp_pass: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[allow(non_snake_case)]
struct Report {
    ANDROID_VERSION: String,
    APP_VERSION_CODE: u64,
    APP_VERSION_NAME: String,
    CUSTOM_DATA: HashMap<String, Value>,
    PACKAGE_NAME: String,
    REPORT_ID: String,
    STACK_TRACE: String,
}

struct ReportHandler {
    pub config: Config,
}

impl middleware::Handler for ReportHandler {
    fn handle(&self, req: &mut Request) -> IronResult<Response> {
        println!("Incoming report...");

        // Get raw body
        let mut payload = String::new();
        match req.body.read_to_string(&mut payload) {
            Ok(_) => {},
            Err(e) => {
                println!("Error: Could not read body to string: {}", e);
                return Ok(Response::with(status::InternalServerError));
            },
        };

        // Store report to file
        match OpenOptions::new()
                          .create(true)
                          .append(true)
                          .open("crashes.txt") {
            Ok(mut file) => if let Err(e) = writeln!(file, "{}", &payload) {
                println!("Error: Could not write crash to file: {:?}", e);
                return Ok(Response::with(status::InternalServerError));
            },
            Err(e) => {
                println!("Error: Could not open crash log file: {:?}", e);
                return Ok(Response::with(status::InternalServerError));
            }
        };
        println!("  -> Saved to crash log file");

        // Parse report
        let report: Report = match serde_json::from_str(&payload) {
            Ok(r) => r,
            Err(e) => {
                println!("Could not parse report: {:?}", e);
                return Ok(Response::with(status::InternalServerError));
            },
        };
        println!("  -> Parsed report");
        println!("  -> Report ID is {}", report.REPORT_ID);

        // Create and send e-mail
        let mut email_text = String::new();
        email_text.push_str("A new crash happened:\r\n\r\n");
        email_text.push_str(&format!("- Report ID: {}\r\n", report.REPORT_ID));
        email_text.push_str(&format!("- Version: {} ({})\r\n", report.APP_VERSION_NAME, report.APP_VERSION_CODE));
        email_text.push_str(&format!("- Android version: {}\r\n", report.ANDROID_VERSION));
        if !report.CUSTOM_DATA.is_empty() {
            email_text.push_str("\r\nCustom data:\r\n\r\n");
            for (key, val) in &report.CUSTOM_DATA {
                email_text.push_str(&format!("  {} = {}\r\n", &key, &val));
            }
        }
        email_text.push_str(&format!("\r\nStack trace:\r\n\r\n{}", report.STACK_TRACE));
        let email_result = Message::builder()
            .from(self.config.email_from.parse().unwrap())
            .to(self.config.email_to.parse().unwrap())
            .subject(&format!("New crash of {} ({})", report.PACKAGE_NAME, report.APP_VERSION_NAME))
            .body(&email_text);
        match email_result {
            Ok(email) => {
                let mut tls_config = rustls::ClientConfig::default();
                tls_config.enable_tickets = false;
                tls_config.root_store.add_server_trust_anchors(&webpki_roots::TLS_SERVER_ROOTS);
                let transport = SmtpTransport::builder(&*self.config.smtp_host)
                    .port(self.config.smtp_port)
                    .credentials(Credentials::new(self.config.smtp_user.clone(), self.config.smtp_pass.clone()))
                    .tls(Tls::Required(TlsParameters::new(
                        self.config.smtp_host.clone(),
                        tls_config
                    )))
                    .build();
                match transport.send(&email) {
                    Ok(_) => {},
                    Err(e) => {
                        println!("Could not send email: {:?}", e);
                        return Ok(Response::with(status::InternalServerError));
                    },
                };
            },
            Err(e) => {
                println!("Could not prepare email: {:?}", e);
                return Ok(Response::with(status::InternalServerError));
            }
        }
        println!("  -> Sent report e-mail");

        Ok(Response::with(status::Ok))
    }
}

fn main() {
    env_logger::init();

    // Load config
    let file = File::open("config.json").unwrap();
    let config: Config = serde_json::from_reader(&file).unwrap();

    let mut router = Router::new();
    router.post("/report", ReportHandler { config: config.clone() }, "report");
    println!("Starting server on {}:{}...", config.host, config.port);
    let mut iron = Iron::new(router);
    iron.threads = 4;
    iron.http(&format!("{}:{}", config.host, config.port)).unwrap();
}
