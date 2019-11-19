#[macro_use]
extern crate log;

use std::time::{Duration, Instant};
use actix::fut;
use actix::prelude::*;
use actix_broker::BrokerIssue;
use actix_files::Files;
use actix_web::{web, App, Error, HttpRequest, HttpResponse, HttpServer};
use actix_web_actors::ws;
use std::fs::File;
use std::io::prelude::*;
use serde::Deserialize;

mod server;
use server::*;
use regex::Regex;

/// How often heartbeat pings are sent
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
/// How long before lack of client response causes a timeout
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Deserialize)]
struct Info {
    f: String,
}

fn is_room_valid(room: &str) -> bool {
    return room == "demo";
}

fn ws_route(room: web::Path<String>, req: HttpRequest, stream: web::Payload) -> Result<HttpResponse, Error> {
    info!("Route: ws/{}", room);
    ws::start(WsSession::new(room.to_string()), &req, stream)
}

fn main() -> std::io::Result<()> {
    let sys = actix::System::new("ninjametal");
    simple_logger::init_with_level(log::Level::Info).unwrap();

    HttpServer::new(|| {
        App::new()
            .service(web::resource("/ws/{room}").route(web::get().to(ws_route)))
            .service(Files::new("/", "wwwroot/").index_file("index.html"))
    })
        .bind("127.0.0.1:8663")
      //  .bind("192.168.1.149:1337")
        .unwrap()
        .start();

    info!("Started http server");
    sys.run()
}





//#[derive(Default)]
struct WsSession {
    id: usize,
    room: String,
    name: Option<String>,
    hb: Instant,
}

impl WsSession {
    fn new(name: String) -> WsSession {
        WsSession {
            id: 0, // will get an id once they have joined a room
            room : "rustdudes".to_string(),
            name : Some(name),
            hb: Instant::now()
        }
    }

    /// helper method that sends ping to client every second.
    /// also this method checks heartbeats from client
    fn hb(&self, ctx: &mut <Self as Actor>::Context) {
        ctx.run_interval(HEARTBEAT_INTERVAL, |act, ctx| {
            // check client heartbeats
            if Instant::now().duration_since(act.hb) > CLIENT_TIMEOUT {
                // heartbeat timed out
                println!("Websocket Client heartbeat failed, disconnecting!");

                // stop actor
                ctx.stop();

                // don't try to send a ping
                return;
            }

            ctx.ping("");
        });
    }

    fn join_room(&mut self, room_name: &str, ctx: &mut ws::WebsocketContext<Self>) {
        let room_name = room_name.to_owned();
        // First send a leave message for the current room
        let leave_msg = LeaveRoom(self.room.clone(), self.id);
        // issue_sync comes from having the `BrokerIssue` trait in scope.
        self.issue_system_sync(leave_msg, ctx);
        // Then send a join message for the new room
        let join_msg = JoinRoom(
            room_name.to_owned(),
            self.name.clone(),
            ctx.address().recipient(),
        );

        WsServer::from_registry()
            .send(join_msg)
            .into_actor(self)
            .then(|id, act, _ctx| {
                if let Ok(id) = id {
                    act.id = id;
                    act.room = room_name;
                }

                fut::ok(())
            })
            .spawn(ctx);
    }

    fn list_rooms(&mut self, ctx: &mut ws::WebsocketContext<Self>) {
        WsServer::from_registry()
            .send(ListRooms)
            .into_actor(self)
            .then(|res, _, ctx| {
                if let Ok(rooms) = res {
                    for room in rooms {
                        ctx.text(room);
                    }
                }
                fut::ok(())
            })
            .spawn(ctx);
    }

    fn send_msg(&self, msg: &str) {
        let content = format!(
            "@{} - {}",
            self.name.clone().unwrap_or_else(|| "anon".to_string()),
            msg
        );
        let msg = SendMessage(self.room.clone(), self.id, content);
        // issue_async comes from having the `BrokerIssue` trait in scope.
        self.issue_system_async(msg);
    }
}

impl Actor for WsSession {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        self.hb(ctx);
        self.join_room(self.room.to_owned().as_str(), ctx);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!(
            "WsChatSession closed for {}({}) in room {}",
            self.name.clone().unwrap_or_else(|| "anon".to_string()),
            self.id,
            self.room
        );
    }
}

impl Handler<ChatMessage> for WsSession {
    type Result = ();

    fn handle(&mut self, msg: ChatMessage, ctx: &mut Self::Context) {
        ctx.text(msg.0);
    }
}

impl StreamHandler<ws::Message, ws::ProtocolError> for WsSession {
    fn handle(&mut self, msg: ws::Message, ctx: &mut Self::Context) {
        debug!("WEBSOCKET MESSAGE: {:?}", msg);
        match msg {
            ws::Message::Ping(msg) => {
                self.hb = Instant::now();
                ctx.pong(&msg);
            }
            ws::Message::Pong(_) => {
                self.hb = Instant::now();
            }
            ws::Message::Text(text) => {
                let msg = text.trim();
                if msg.starts_with('/') {
                    let mut command = msg.splitn(2, ' ');
                    match command.next() {
                        Some("/list") => self.list_rooms(ctx),
                        Some("/join") => {
                            if let Some(room_name) = command.next() {
                                self.join_room(room_name, ctx);
                            } else {
                                ctx.text("!!! room name is required");
                            }
                        }
                        Some("/name") => {
                            if let Some(name) = command.next() {
                                self.name = Some(name.to_owned());
                                ctx.text(format!("name changed to: {}", name));
                            } else {
                                ctx.text("!!! name is required");
                            }
                        }
                        _ => ctx.text(format!("!!! unknown command: {:?}", msg)),
                    }
                    return;
                }
                self.send_msg(msg);
            }
            ws::Message::Close(_) => {
                ctx.stop();
            }
            _ => {}
        }
    }
}