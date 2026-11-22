//! What the app gives every plugin, whichever it is (nothing here knows a plugin by name): the targets of a button with what the plugin said of
//! them, the `when` of a button, the way in to a server (`ssh`), and the whole-screen text a plugin shows (`text`: lines in pieces or plain, anchors
//! `]` / `[` move among, keys that call the plugin back).

use super::*;

impl App {
    /// The marked entries of the panel (or the one under the cursor) with the flags a plugin gave them: `(name, is a folder, flags)`.
    pub(super) fn target_flags(&self, plugin: &str) -> Vec<(String, bool, Vec<String>)> {
        let p = self.p();
        let Some(key) = p.place_key() else { return Vec::new() };
        let names: Vec<String> = if p.marked.is_empty() {
            p.current().filter(|e| !e.is_parent()).map(|e| vec![e.name.clone()]).unwrap_or_default()
        } else {
            p.marked.iter().cloned().collect()
        };
        let mine = format!("{plugin}.");
        names
            .into_iter()
            .filter_map(|n| {
                let e = p.entries.iter().find(|e| e.name == n)?;
                let mut flags: Vec<String> = Vec::new();
                for (col, (k, m)) in &p.plugin_flags {
                    if col.starts_with(&mine) && *k == key && let Some(f) = m.get(&n) {
                        flags.extend(f.iter().cloned());
                    }
                }
                Some((n, e.is_dir(), flags))
            })
            .collect()
    }

    /// What the plugin said of the folder of the active panel (`None`: not asked yet, or it says nothing).
    pub(super) fn plugin_state_of(&self, plugin: &str) -> Option<&crate::panel::PluginState> {
        let p = self.p();
        let key = p.place_key()?;
        p.plugin_state.get(plugin).filter(|s| s.key == key)
    }

    /// Does a button's `when` hold now? Conditions on what the plugin said of the folder (`state`) and of the targets (`flags`): `x` the folder has the
    /// flag, `item:x` some target has it, `one:x` the only target has it; a `!` before any of them turns it round.
    pub(super) fn when_ok(&self, a: &crate::plugins::Action) -> bool {
        if a.when.is_empty() {
            return true;
        }
        let state = self.plugin_state_of(&a.source);
        let targets = self.target_flags(&a.source);
        let has = |f: &[String], x: &str| f.iter().any(|y| y == x);
        a.when.iter().all(|c| {
            let (neg, c) = c.strip_prefix('!').map_or((false, c.as_str()), |r| (true, r));
            let r = match c.split_once(':') {
                Some(("item", f)) => targets.iter().any(|(_, _, t)| has(t, f)),
                Some(("one", f)) => targets.len() == 1 && has(&targets[0].2, f),
                _ => state.is_some_and(|s| has(&s.flags, c)),
            };
            r != neg
        })
    }


    /// The ssh way in to a place a plugin serves, as plugins get it in a call's `ssh`: `{"host", "user"?, "port"?, "key"?}` — only where the places
    /// plugin says the place has one.
    pub(super) fn ssh_of(&mut self, remote: &str) -> Option<serde_json::Value> {
        self.place_info.get(remote).map(|i| i["ssh"].clone()).filter(|s| s["host"].is_string())
    }

    /// Leave the viewer.
    pub(super) fn close_viewer(&mut self) {
        self.viewer = false;
        self.view_text = None;
        self.pv_off = 0;
        self.drawer.forget();
    }

    /// The buttons of a whole-screen text a plugin gave (key, label, the key as a char).
    pub(super) fn text_buttons(&self) -> Vec<(String, String, char)> {
        let Some(t) = &self.view_text else { return Vec::new() };
        let mut v = Vec::new();
        if !t.anchors.is_empty() {
            v.push(("]".to_string(), "next".to_string(), ']'));
            v.push(("[".to_string(), "prev".to_string(), '['));
        }
        for k in &t.keys {
            let name = if k.key == '\t' { "Tab".to_string() } else { k.key.to_string() };
            v.push((name, k.label.clone(), k.key));
        }
        v
    }

    /// A key of a whole-screen text: `]` / `[` move among its anchors; a key the plugin gave calls it back (with what line, anchor and note the
    /// text has). Returns whether the key was taken.
    pub(super) fn text_key(&mut self, c: char) -> bool {
        let Some(t) = self.view_text.as_mut() else { return false };
        let n = t.anchors.len();
        if n > 0 && matches!(c, ']' | 'n' | '[' | 'p') {
            let to = if matches!(c, ']' | 'n') { (t.cur + 1).min(n - 1) } else { t.cur.saturating_sub(1) };
            t.cur = to;
            if let Some(i) = t.title.rfind("hunk ") {
                let head = t.title[..i].to_string();
                t.title = format!("{head}hunk {}/{n}", to + 1);
            }
            self.pv_line = Some(t.anchors[to]);
            return true;
        }
        let Some(k) = t.keys.iter().find(|k| k.key == c).cloned() else { return false };
        let Some(from) = t.from.clone() else { return false };
        let arg = serde_json::json!({"anchor": t.cur, "line": t.anchors.get(t.cur), "data": from.data}).to_string();
        let call = PluginCall { plugin: from.plugin, call: k.call, data: Some(arg.clone()) };
        match k.confirm {
            Some(text) => {
                self.dialog = Some(Dialog::Confirm { title: k.label, lines: text.lines().map(str::to_string).collect(), action: ConfirmAction::Plugin(call) });
            }
            None => self.plugin_call(&call, Some(&arg)),
        }
        true
    }

    /// The colour of a line of a diff / commit text.
    pub(super) fn text_line_style(l: &Look, line: &str) -> Style {
        let base = Style { bg: l.panel.bg, ..l.panel };
        let colour = if line.starts_with("+++") || line.starts_with("---") {
            Some("yellow")
        } else if line.starts_with('+') {
            Some("green")
        } else if line.starts_with('-') {
            Some("red")
        } else if line.starts_with("@@") {
            Some("cyan")
        } else if line.starts_with("diff ") || line.starts_with("index ") || line.starts_with("new file") || line.starts_with("deleted file") || line.starts_with("similarity") || line.starts_with("rename ") {
            Some("comment")
        } else if line.starts_with("commit ") {
            Some("yellow")
        } else if line.starts_with("Author:") || line.starts_with("Date:") {
            Some("blue")
        } else {
            None
        };
        match colour {
            Some(c) => tint(l, c, base, base.bg),
            None => base,
        }
    }

    /// The text of `view_text` on the whole area: its title on a bar, then the lines from the offset `pv_off` (kept inside the text); a long line is
    /// wrapped. A line in pieces has each in its colour; the current anchor's line is lit.
    pub(super) fn draw_text_view(&mut self, cv: &mut Canvas, l: &Look, r: Rect) {
        let Some(t) = &self.view_text else { return };
        let (f_bar, f_name, _) = bar_styles(l, true);
        cv.fill(Rect { x: r.x, y: r.y, w: r.w, h: 1 }, f_bar);
        let x = cv.put(r.x, r.y, " ", f_name, r.x + r.w);
        cv.put(x, r.y, &format!(" {} ", truncate(&t.title, r.w.saturating_sub(4) as usize)), f_name, r.x + r.w);
        let h = r.h.saturating_sub(1) as usize;
        let width = r.w.saturating_sub(2).max(10) as usize;
        let mut rows: Vec<(usize, String)> = Vec::new();
        let mut first_row: Vec<usize> = Vec::with_capacity(t.lines.len());
        for (i, line) in t.lines.iter().enumerate() {
            first_row.push(rows.len());
            for seg in wrap_line(&line.replace('\t', "    "), width) {
                rows.push((i, seg));
            }
        }
        if let Some(li) = self.pv_line.take() {
            self.pv_off = first_row.get(li).copied().unwrap_or(0);
        }
        let max = rows.len().saturating_sub(h);
        let off = self.pv_off.min(max);
        let cur = t.anchors.get(t.cur).copied();
        for (row, (li, seg)) in rows.iter().skip(off).take(h).enumerate() {
            let mut st = if t.diff_style { Self::text_line_style(l, &t.lines[*li]) } else { Style { bg: l.panel.bg, ..l.panel } };
            if cur == Some(*li) {
                st = Style { fg: l.sel[0].fg, bg: l.sel[0].bg, attrs: l.sel[0].attrs, ..st };
            }
            let y = r.y + 1 + row as u16;
            cv.fill(Rect { x: r.x, y, w: r.w, h: 1 }, st);
            let spans = t.spans.get(*li).filter(|s| !s.is_empty() && first_row[*li] == off + row);
            match spans {
                Some(spans) => {
                    let mut x = r.x + 1;
                    for (text, colour) in spans {
                        let cst = if colour.is_empty() { st } else { tint(l, colour, st, st.bg) };
                        x = cv.put(x, y, text, cst, r.x + r.w);
                    }
                }
                None => {
                    cv.put(r.x + 1, y, seg, st, r.x + r.w);
                }
            }
        }
        self.pv_off = off;
    }
}

/// A line cut into rows of at most `width` cells: the rest of a long line goes on the next rows, two cells in.
pub(super) fn wrap_line(line: &str, width: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut w = 0usize;
    let mut limit = width;
    for c in line.chars() {
        let mut b = [0u8; 4];
        let cw = str_width(c.encode_utf8(&mut b));
        if w + cw > limit && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
            cur.push_str("  ");
            w = 2;
            limit = width;
        }
        cur.push(c);
        w += cw;
    }
    out.push(cur);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_line_is_wrapped_and_the_rest_is_a_little_in() {
        assert_eq!(wrap_line("short", 10), vec!["short"]);
        assert_eq!(wrap_line("", 10), vec![""]);
        let rows = wrap_line("abcdefghijklmnopqrstuvwxyz", 10);
        assert_eq!(rows[0], "abcdefghij");
        assert!(rows[1].starts_with("  klmnopq") && rows.iter().all(|r| str_width(r) <= 10));
        assert_eq!(rows.iter().map(|r| r.trim_start()).collect::<String>().replace(' ', ""), "abcdefghijklmnopqrstuvwxyz");
        // wide characters count for two cells
        let rows = wrap_line("ааааааааааааааааааааа", 10);
        assert!(rows.iter().all(|r| str_width(r) <= 10), "{rows:?}");
    }
}
