//! Replies required by ConPTY and full-screen applications.
#[derive(Default)]
pub struct Replies {
    pub bytes: Vec<u8>,
    pub win32_input: bool,
}
impl vt100::Callbacks for Replies {
    fn unhandled_csi(&mut self, screen: &mut vt100::Screen, prefix: Option<u8>, second: Option<u8>, params: &[&[u16]], command: char) {
        let first = params.first().and_then(|p| p.first()).copied().unwrap_or(0);
        let reply = match (prefix, second, command, first) {
            (None | Some(b'?'), None, 'n', 6) => {
                let (row, col) = screen.cursor_position();
                format!("\x1b[{}{};{}R", if prefix.is_some() { "?" } else { "" }, row + 1, col + 1)
            }
            (None, None, 'n', 5) => "\x1b[0n".into(),
            (None, None, 'c', 0) => "\x1b[?1;2c".into(),
            (Some(b'>'), None, 'c', 0) => "\x1b[>0;1;0c".into(),
            (Some(b'?'), None, 'u', _) => "\x1b[?0u".into(),
            (Some(b'?'), None, 'h' | 'l', _) => {
                if params.iter().any(|p| p.first() == Some(&9001)) { self.win32_input = command == 'h'; }
                String::new()
            }
            (None, None, 't', 18) => {
                let (rows, cols) = screen.size();
                format!("\x1b[8;{rows};{cols}t")
            }
            _ => String::new(),
        };
        self.bytes.extend_from_slice(reply.as_bytes());
    }
    fn unhandled_osc(&mut self, _: &mut vt100::Screen, params: &[&[u8]]) {
        match params {
            [b"10", b"?"] => self.bytes.extend_from_slice(b"\x1b]10;rgb:dfdf/e8e8/efef\x1b\\"),
            [b"11", b"?"] => self.bytes.extend_from_slice(b"\x1b]11;rgb:1111/1616/2929\x1b\\"),
            _ => {},
        }
    }
}
