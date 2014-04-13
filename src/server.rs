use grain_capnp::{PowerboxCapability, UiView, UiSession};
use web_session_capnp::{WebSession};

use collections::hashmap::HashMap;
use capnp::capability::{ClientHook, FromServer};
use capnp::AnyPointer;
use capnp_rpc::rpc::{RpcConnectionState, SturdyRefRestorer};
use capnp_rpc::capability::{LocalClient};

use sqlite3;

pub struct UiViewImpl;

impl PowerboxCapability::Server for UiViewImpl {
    fn get_powerbox_info(&mut self, context : PowerboxCapability::GetPowerboxInfoContext) {
        context.done()
    }
}

impl UiView::Server for UiViewImpl {
    fn get_view_info(&mut self, context : UiView::GetViewInfoContext) {
        context.done()
    }

    fn new_session(&mut self, mut context : UiView::NewSessionContext) {
        println!("asked for a new session!");
        let (_, results) = context.get();

        let client : WebSession::Client = FromServer::new(None::<LocalClient>, ~WebSessionImpl::new());
        // we need to do this dance to upcast.
        results.set_session(UiSession::Client { client : client.client});

        context.done()
    }
}

pub struct WebSessionImpl {
    db : sqlite3::Database,
}

impl WebSessionImpl {
    pub fn new() -> WebSessionImpl {
        WebSessionImpl {
            db : sqlite3::open("/var/data.db").unwrap(),
        }
    }
}

impl UiSession::Server for WebSessionImpl {

}

static main_css : &'static str =
    "body { font-family: Helvetica, Sans, Arial;
            font-size: medium;
             margin-left: auto;
             margin-right: auto;
             width: 600px;
             text-align: center;
     }
    .word {
        text-align: center;
        font-size: 500%;
     }
     ";


static header : &'static str =
  r#"<head><title> acronymy </title><link rel="stylesheet" type="text/css" href="main.css" >
 <meta http-equiv="Content-Type" content="text/html;charset=utf-8" >
  </head>"#;

fn html_body(body :&str) -> ~str {
    format!("<html>{}<body>{}</body></html>", header, body)
}

struct Path {
    path : ~str,
    query : HashMap<~str, ~str>,
}

impl Path {
    fn new() -> Path {
        Path { path : ~"", query : HashMap::new() }
    }
}

fn parse_path(path : &str) -> Path {
    let mut result = Path::new();

    let v : ~[&str] = path.splitn('?', 2).collect();
    if v.len() == 0 {
        return result;
    }
    result.path = v[0].into_owned();
    if v.len() == 1 {
        return result;
    }
    for attr in v[1].split('&') {
        let a : ~[&str] = attr.splitn('=', 2).collect();
        if a.len() == 2 {
            result.query.insert(a[0].into_owned(),
                                a[1].into_owned());
        }
    }
    return result;
}

impl WebSessionImpl {

    fn checked<T>(&self, result : sqlite3::SqliteResult<T>) -> T {
        match result {
            Err(e) => {fail!("database error: {}, {:?}", e, self.db.get_errmsg()) }
            Ok(v) => { return v; }
        }
    }

    fn is_word(&self, word : &str) -> sqlite3::SqliteResult<bool> {

        // TODO sanitize `word`
        let cursor = try!(self.db.prepare(
            format!("SELECT * FROM Words WHERE Word = \"{}\";", word),
            &None));
        println!("got the cursor");
        return Ok(try!(cursor.step_row()).is_some());
    }

    fn validate_def(&self, word : &str, definition : &[&str]) -> sqlite3::SqliteResult<bool> {

        if definition.len() != word.len() { return Ok(false); }

        let mut idx = 0;
        for &d in definition.iter() {
            if !(try!(self.is_word(d)) && d.len() > 0 && d[0] == word[idx]) {
                return Ok(false);
            }

            idx += 1;
        }

        return Ok(true);
    }

    fn write_def(&self, word : &str, definition : &[&str]) -> sqlite3::SqliteResult<()> {
        let mut query = StrBuf::new();
        query.push_str(format!("BEGIN; DELETE FROM Definitions WHERE Definee =\"{}\"; ", word));
        query.push_str("INSERT INTO Definitions(Definee, Idx, Definer) VALUES");
        let mut idx = 0;
        for &d in definition.iter() {
            if idx != 0 { query.push_str(","); }
            query.push_str(format!("(\"{}\", {}, \"{}\")", word, idx, d));
            idx += 1;
        }
        query.push_str("; COMMMIT;");

        println!("query: {}", query);

        try!(self.db.exec(query.as_slice()));

        return Ok(());
    }

    fn get_def(&self, word : &str) -> sqlite3::SqliteResult<~str> {

        let cursor = try!(self.db.prepare(
            format!("SELECT * FROM Definitions WHERE Definee = \"{}\";", word),
            &None));

        let mut map = HashMap::<int, ~str>::new();

        loop {
            match try!(cursor.step_row()) {
                None => break,
                Some(row) => {
                    let definer = match row.get(&~"Definer") { &sqlite3::Text(ref t) => t.clone(), _ => fail!(), };
                    let idx = match row.get(&~"Idx") { &sqlite3::Integer(ref i) => i.clone(), _ => fail!(), };

                    map.insert(idx, definer);
                }
            }
        }

        if map.len() != word.len() {
            return Ok(~"<div>this word has no definition yet</div>");
        } else {

            let mut result = StrBuf::new();
            result.push_str("<div>");
            for idx in range::<int>(0, word.len() as int) {
                let definer : &str = map.get(&idx).as_slice();
                result.push_str(format!(" <a href=\"define?word={word}\">{word}</a> ", word=definer));
            }
            result.push_str("</div>");
            return Ok(result.into_owned());
        }
    }

}

impl WebSession::Server for WebSessionImpl {
    fn get(&mut self, mut context : WebSession::GetContext) {
        println!("GET");
        let (params, results) = context.get();
        let raw_path = params.get_path();
        let content = results.init_content();
        content.set_mime_type("text/html");

        let path = parse_path(raw_path);
        println!("path = {}", raw_path);

        if raw_path == "main.css" {
            content.get_body().set_bytes(main_css.as_bytes())
        } else if path.path.as_slice() == "define" {
            let word : ~str = path.query.get(&~"word").clone();

            if !self.checked(self.is_word(word)) {
                content.get_body().set_bytes(
                    html_body(
                        "<div> that's not a word </div>
                           <form action=\"define\" method=\"get\">
                           <input name=\"word\"/><button>go</button></form>").as_bytes());
                return context.done();
            } else {

            }

            match path.query.find(&~"definition") {
                None => {
                    let def_div = self.checked(self.get_def(word));

                    content.get_body().set_bytes(
                        html_body(
                            format!(
                                "<div class=\"word\">{word}</div>
                                 {def}
                                 <form action=\"define\" method=\"get\">
                                 <input name=\"word\" value=\"{word}\" type=\"hidden\"/>
                                 <input name=\"definition\"/><button>define</button></form>",
                                word=word, def=def_div)).as_bytes());


                }
                Some(def_query) => {

                    let definition : ~[&str] = def_query.split('+').collect();

                    if self.checked(self.validate_def(word, definition)) {

                        self.write_def(word, definition);

                        let def_div = self.checked(self.get_def(word));

                        content.get_body().set_bytes(
                        html_body(
                            format!(
                                "<div class=\"word\">{word}</div>
                                 {def}
                                 <form action=\"define\" method=\"get\">
                                 <input name=\"word\" value=\"{word}\" type=\"hidden\"/>
                                 <input name=\"definition\"/><button>define</button></form>",
                                word=word, def=def_div)).as_bytes());




                    } else {

                        let def_div = self.checked(self.get_def(word));

                        content.get_body().set_bytes(
                        html_body(
                            format!(
                                "<div class=\"word\">{word}</div>
                                 {def}
                                 <div>invalid definition</div>
                                 <form action=\"define\" method=\"get\">
                                 <input name=\"word\" value=\"{word}\" type=\"hidden\"/>
                                 <input name=\"definition\"/><button>define</button></form>",
                                word=word, def=def_div)).as_bytes());

                    }
                }
            }


        } else {
            content.get_body().set_bytes(
                html_body(
                    "<form action=\"define\" method=\"get\">
                     <input name=\"word\"/><button>go</button></form>").as_bytes());
        }
        context.done()
    }
    fn post(&mut self, context : WebSession::PostContext) {
        println!("POST");
        context.done()
    }
    fn put(&mut self, context : WebSession::PutContext) {
        println!("PUT");
        context.done()
    }
    fn delete(&mut self, context : WebSession::DeleteContext) {
        println!("DELETE");
        context.done()
    }
    fn open_web_socket(&mut self, context : WebSession::OpenWebSocketContext) {
        println!("OPEN WEB SOCKET");
        context.done()
    }
}



pub struct FdStream {
    inner : ~::std::rt::rtio::RtioFileStream:Send,
}

impl FdStream {
    pub fn new(fd : ::libc::c_int) -> ::std::io::IoResult<FdStream> {
        ::std::rt::rtio::LocalIo::maybe_raise(|io| {
            Ok (FdStream { inner : io.fs_from_raw_fd(fd, ::std::rt::rtio::DontClose) })
        })
    }
}

impl Reader for FdStream {
    fn read(&mut self, buf : &mut [u8]) -> ::std::io::IoResult<uint> {
        self.inner.read(buf).map(|i| i as uint)
    }
}

impl Writer for FdStream {
    fn write(&mut self, buf : &[u8]) -> ::std::io::IoResult<()> {
        self.inner.write(buf)
    }
}

pub struct Restorer;

impl SturdyRefRestorer for Restorer {
    fn restore(&self, obj_id : AnyPointer::Reader) -> Option<~ClientHook:Send> {
        if obj_id.is_null() {
            let client : UiView::Client = FromServer::new(None::<LocalClient>, ~UiViewImpl);
            Some(client.client.hook)
        } else {
            None
        }
    }
}

pub fn main() -> ::std::io::IoResult<()> {

    let args = ::std::os::args();

    if args.len() == 4 && args[1].as_slice() == "--init" {
        println!("initializing...");
        let initdb_path = ::std::path::Path::new(args[2].as_slice());
        let proddb_path = ::std::path::Path::new(args[3].as_slice());
        println!("copying database from {} to {}", args[2], args[3]);
        try!(::std::io::fs::copy(&initdb_path, &proddb_path));
        println!("success!");
    }

    let ifs = try!(FdStream::new(3));
    let ofs = try!(FdStream::new(3));

    let connection_state = RpcConnectionState::new();
    connection_state.run(ifs, ofs, Restorer);

    Ok(())
}



