use std::collections::BTreeMap;

#[derive(Clone)]
struct Line {
    text: String,
    source_line: usize,
    expansion_id: Option<usize>,
}

#[derive(Clone)]
struct Macro {
    name: String,
    args: Vec<Line>,
}

pub(crate) struct PreprocessedSource {
    pub(crate) text: String,
    pub(crate) source_lines: Vec<usize>,
}

pub(crate) fn preprocess(source: &str) -> Result<PreprocessedSource, crate::AssemblyError> {
    let lines = source
        .lines()
        .enumerate()
        .map(|(i, text)| Line {
            text: text.to_string(),
            source_line: i + 1,
            expansion_id: None,
        })
        .collect::<Vec<_>>();
    let mut state = State {
        macros: BTreeMap::new(),
        symbols: BTreeMap::new(),
        expansion_id: 0,
    };
    let mut output = Vec::new();
    state.process(&lines, &mut output, 0, 0)?;
    let text = output
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let source_lines = output.iter().map(|line| line.source_line).collect();
    Ok(PreprocessedSource { text, source_lines })
}

struct State {
    macros: BTreeMap<String, Macro>,
    symbols: BTreeMap<String, i64>,
    expansion_id: usize,
}

enum Flow {
    Continue,
    Break,
}

const MAX_EXPANSIONS: usize = 100_000;

impl State {
    fn process(
        &mut self,
        lines: &[Line],
        output: &mut Vec<Line>,
        depth: usize,
        repeat_depth: usize,
    ) -> Result<Flow, crate::AssemblyError> {
        if depth > 64 {
            return Err(pre_error(
                lines.first().map_or(1, |l| l.source_line),
                "macro expansion exceeded 64 nested calls",
            ));
        }
        let mut conditionals: Vec<(bool, bool, bool)> = Vec::new();
        let mut active = true;
        let mut index = 0;
        while index < lines.len() {
            let line = &lines[index];
            let resolved = resolve_expansion_id(&line.text, line.expansion_id);
            let clean = strip_comment(&resolved).trim();
            if clean.is_empty() {
                index += 1;
                continue;
            }
            let (head, rest) = split_head(clean);
            let keyword = head.to_ascii_uppercase();
            match keyword.as_str() {
                "IF" | "IFDEF" | "IFNDEF" | "IFN" | "IFB" | "IFNB" => {
                    let parent = active;
                    let cond = if !parent {
                        false
                    } else {
                        self.condition(&keyword, rest, line.source_line)?
                    };
                    conditionals.push((parent, cond, cond));
                    active = parent && cond;
                }
                "ELIF" | "ELSE" => {
                    let Some((parent, taken, _)) = conditionals.last_mut() else {
                        return Err(pre_error(
                            line.source_line,
                            format!("{keyword} without matching IF"),
                        ));
                    };
                    if keyword == "ELSE" {
                        let next = *parent && !*taken;
                        *taken = true;
                        conditionals.last_mut().unwrap().2 = next;
                        active = next;
                    } else {
                        let next =
                            *parent && !*taken && self.condition("IF", rest, line.source_line)?;
                        *taken |= next;
                        conditionals.last_mut().unwrap().2 = next;
                        active = next;
                    }
                }
                "ENDC" => {
                    if conditionals.pop().is_none() {
                        return Err(pre_error(line.source_line, "ENDC without matching IF"));
                    }
                    active = conditionals.last().map_or(true, |c| c.0 && c.2);
                }
                "REPT" | "FOR" => {
                    let end = block_end(lines, index)?;
                    if active {
                        let body = &lines[index + 1..end];
                        if keyword == "REPT" {
                            self.repeat(body, rest, output, depth, repeat_depth, line.source_line)?;
                        } else {
                            self.repeat_for(
                                body,
                                rest,
                                output,
                                depth,
                                repeat_depth,
                                line.source_line,
                            )?;
                        }
                    }
                    index = end + 1;
                    continue;
                }
                "ENDR" => {
                    return Err(pre_error(
                        line.source_line,
                        "ENDR without matching REPT or FOR",
                    ));
                }
                "BREAK" if active => {
                    if repeat_depth == 0 {
                        return Err(pre_error(
                            line.source_line,
                            "BREAK outside a REPT or FOR block",
                        ));
                    }
                    return Ok(Flow::Break);
                }
                _ if !active => {}
                "DEF" | "REDEF" => self.define_symbol(rest, line.source_line)?,
                _ if rest.to_ascii_uppercase().starts_with("EQU ") => {
                    self.define_symbol(&format!("{head} EQU {}", &rest[4..]), line.source_line)?;
                }
                "MACRO" => {
                    let name = rest
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .trim_end_matches(':');
                    if name.is_empty() {
                        return Err(pre_error(line.source_line, "expected a macro name"));
                    }
                    let end = lines[index + 1..]
                        .iter()
                        .position(|item| {
                            let (head, _) = split_head(strip_comment(&item.text).trim());
                            head.eq_ignore_ascii_case("ENDM")
                        })
                        .map(|offset| index + 1 + offset)
                        .ok_or_else(|| {
                            pre_error(line.source_line, format!("macro `{name}` has no ENDM"))
                        })?;
                    let key = name.to_ascii_uppercase();
                    if self.macros.contains_key(&key) {
                        return Err(pre_error(
                            line.source_line,
                            format!("macro `{name}` is defined more than once"),
                        ));
                    }
                    self.macros.insert(
                        key,
                        Macro {
                            name: name.to_string(),
                            args: lines[index + 1..end].to_vec(),
                        },
                    );
                    index = end + 1;
                    continue;
                }
                "ENDM" => return Err(pre_error(line.source_line, "ENDM without matching MACRO")),
                "SHIFT" => {
                    return Err(pre_error(
                        line.source_line,
                        "SHIFT is only valid inside a macro",
                    ));
                }
                _ => {
                    if let Some((name, tail, label)) = invocation(clean) {
                        if let Some(mac) = self.macros.get(&name.to_ascii_uppercase()).cloned() {
                            let args = split_args(tail);
                            let expansion_id = self.next_expansion_id(line.source_line)?;
                            let mut expanded =
                                expand_macro(&mac, &args, expansion_id, line.source_line)?;
                            if let Some(label) = label {
                                if let Some(first) = expanded.first_mut() {
                                    first.text = format!("{label}: {}", first.text);
                                } else {
                                    expanded.push(Line {
                                        text: format!("{label}:"),
                                        source_line: line.source_line,
                                        expansion_id: None,
                                    });
                                }
                            }
                            if matches!(
                                self.process(&expanded, output, depth + 1, repeat_depth)?,
                                Flow::Break
                            ) {
                                return Ok(Flow::Break);
                            }
                        } else {
                            let mut emitted = line.clone();
                            emitted.text = resolved;
                            emitted.text = evaluate_line(
                                &substitute_symbols(&emitted.text, &self.symbols),
                                &self.symbols,
                            );
                            self.emit(output, emitted)?;
                        }
                    } else {
                        let mut emitted = line.clone();
                        emitted.text = resolved;
                        emitted.text = evaluate_line(
                            &substitute_symbols(&emitted.text, &self.symbols),
                            &self.symbols,
                        );
                        self.emit(output, emitted)?;
                    }
                }
            }
            index += 1;
        }
        if let Some((_, _, _)) = conditionals.last() {
            let line = lines.last().map_or(1, |l| l.source_line);
            return Err(pre_error(line, "unterminated conditional block"));
        }
        Ok(Flow::Continue)
    }

    fn repeat(
        &mut self,
        body: &[Line],
        count_expr: &str,
        output: &mut Vec<Line>,
        depth: usize,
        repeat_depth: usize,
        source_line: usize,
    ) -> Result<(), crate::AssemblyError> {
        let count = self.eval(count_expr, source_line)?;
        if count < 0 {
            return Err(pre_error(source_line, "REPT count must not be negative"));
        }
        if count > MAX_EXPANSIONS as i64 {
            return Err(pre_error(
                source_line,
                "REPT count exceeds the 100000 iteration limit",
            ));
        }
        for _ in 0..count {
            let expansion_id = self.next_expansion_id(source_line)?;
            let iteration = body
                .iter()
                .cloned()
                .map(|mut line| {
                    line.expansion_id = Some(expansion_id);
                    line
                })
                .collect::<Vec<_>>();
            if matches!(
                self.process(&iteration, output, depth, repeat_depth + 1)?,
                Flow::Break
            ) {
                break;
            }
        }
        Ok(())
    }

    fn repeat_for(
        &mut self,
        body: &[Line],
        args_text: &str,
        output: &mut Vec<Line>,
        depth: usize,
        repeat_depth: usize,
        source_line: usize,
    ) -> Result<(), crate::AssemblyError> {
        let args = split_args(args_text);
        if !(2..=4).contains(&args.len()) {
            return Err(pre_error(
                source_line,
                "FOR expects a variable and stop, or variable, start, stop, and optional step",
            ));
        }
        let name = args[0].trim().to_ascii_uppercase();
        if name.is_empty()
            || !name.chars().enumerate().all(|(i, c)| {
                c == '_' || c.is_ascii_alphanumeric() && (i > 0 || !c.is_ascii_digit())
            })
        {
            return Err(pre_error(source_line, "FOR variable must be an identifier"));
        }
        let (mut value, stop, step) = match args.len() {
            2 => (0, self.eval(&args[1], source_line)?, 1),
            3 => (
                self.eval(&args[1], source_line)?,
                self.eval(&args[2], source_line)?,
                1,
            ),
            4 => (
                self.eval(&args[1], source_line)?,
                self.eval(&args[2], source_line)?,
                self.eval(&args[3], source_line)?,
            ),
            _ => unreachable!(),
        };
        if step == 0 {
            return Err(pre_error(source_line, "FOR step must not be zero"));
        }
        let mut iterations = 0usize;
        while if step > 0 { value < stop } else { value > stop } {
            if iterations >= MAX_EXPANSIONS {
                return Err(pre_error(
                    source_line,
                    "FOR exceeds the 100000 iteration limit",
                ));
            }
            self.symbols.insert(name.clone(), value);
            let expansion_id = self.next_expansion_id(source_line)?;
            let iteration = body
                .iter()
                .cloned()
                .map(|mut line| {
                    line.expansion_id = Some(expansion_id);
                    line
                })
                .collect::<Vec<_>>();
            if matches!(
                self.process(&iteration, output, depth, repeat_depth + 1)?,
                Flow::Break
            ) {
                return Ok(());
            }
            value = value.checked_add(step).ok_or_else(|| {
                pre_error(
                    source_line,
                    "FOR value overflowed the supported integer range",
                )
            })?;
            iterations += 1;
        }
        self.symbols.insert(name, value);
        Ok(())
    }

    fn emit(&self, output: &mut Vec<Line>, line: Line) -> Result<(), crate::AssemblyError> {
        if output.len() >= MAX_EXPANSIONS {
            return Err(pre_error(
                line.source_line,
                "preprocessing exceeded 100000 expanded lines",
            ));
        }
        output.push(line);
        Ok(())
    }

    fn next_expansion_id(&mut self, source_line: usize) -> Result<usize, crate::AssemblyError> {
        if self.expansion_id >= MAX_EXPANSIONS {
            return Err(pre_error(
                source_line,
                "preprocessing exceeded 100000 expansions",
            ));
        }
        self.expansion_id += 1;
        Ok(self.expansion_id)
    }

    fn condition(
        &self,
        keyword: &str,
        expr: &str,
        line: usize,
    ) -> Result<bool, crate::AssemblyError> {
        let name = expr.trim().trim_matches('"').to_ascii_uppercase();
        match keyword {
            "IFDEF" => Ok(self.symbols.contains_key(&name) || self.macros.contains_key(&name)),
            "IFNDEF" => Ok(!self.symbols.contains_key(&name) && !self.macros.contains_key(&name)),
            "IFB" => Ok(expr.trim().is_empty()),
            "IFNB" => Ok(!expr.trim().is_empty()),
            "IFN" => Ok(self.eval(expr, line)? == 0),
            _ => Ok(self.eval(expr, line)? != 0),
        }
    }

    fn define_symbol(&mut self, rest: &str, line: usize) -> Result<(), crate::AssemblyError> {
        let upper = rest.to_ascii_uppercase();
        let split = upper
            .find(" EQU ")
            .map(|i| (i, i + 5))
            .or_else(|| rest.find(',').map(|i| (i, i + 1)))
            .ok_or_else(|| pre_error(line, "expected `DEF name EQU expression`"))?;
        let (name, value) = (&rest[..split.0], &rest[split.1..]);
        let name = name.trim().trim_end_matches(':').to_ascii_uppercase();
        if name.is_empty() {
            return Err(pre_error(line, "symbol name is empty"));
        }
        let value = self.eval(value.trim(), line)?;
        self.symbols.insert(name, value);
        Ok(())
    }

    fn eval(&self, expr: &str, line: usize) -> Result<i64, crate::AssemblyError> {
        let mut parser = ExprParser::new(expr, &self.symbols);
        parser.parse().map_err(|message| pre_error(line, message))
    }
}

fn expand_macro(
    mac: &Macro,
    args: &[String],
    id: usize,
    call_line: usize,
) -> Result<Vec<Line>, crate::AssemblyError> {
    let mut result = Vec::new();
    let mut args = args.to_vec();
    for line in &mac.args {
        let clean = strip_comment(&line.text).trim();
        let (head, rest) = split_head(clean);
        if head.eq_ignore_ascii_case("SHIFT") {
            let count = rest.parse::<usize>().unwrap_or(1);
            if count > args.len() {
                return Err(pre_error(
                    call_line,
                    format!("SHIFT {count} exceeds the remaining macro arguments"),
                ));
            }
            args.drain(..count);
            continue;
        }
        let mut text = String::new();
        let chars = line.text.chars().collect::<Vec<_>>();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '\\' && i + 1 < chars.len() {
                let c = chars[i + 1];
                match c {
                    '1'..='9' => {
                        let n = c.to_digit(10).unwrap() as usize;
                        let value = args.get(n - 1).ok_or_else(|| {
                            pre_error(
                                call_line,
                                format!("macro `{}` references missing argument \\{n}", mac.name),
                            )
                        })?;
                        text.push_str(value);
                        i += 2;
                        continue;
                    }
                    '#' => {
                        text.push_str(&args.join(", "));
                        i += 2;
                        continue;
                    }
                    '@' => {}
                    ',' => {
                        text.push(',');
                        i += 2;
                        continue;
                    }
                    _ => {}
                }
            }
            text.push(chars[i]);
            i += 1;
        }
        result.push(Line {
            text,
            source_line: call_line,
            expansion_id: Some(id),
        });
    }
    Ok(result)
}

fn split_head(line: &str) -> (&str, &str) {
    let end = line.find(char::is_whitespace).unwrap_or(line.len());
    (&line[..end], line[end..].trim())
}

fn block_end(lines: &[Line], start: usize) -> Result<usize, crate::AssemblyError> {
    let opener = split_head(strip_comment(&lines[start].text).trim()).0;
    let mut depth = 1usize;
    let mut in_macro = false;
    for (index, line) in lines.iter().enumerate().skip(start + 1) {
        let head = split_head(strip_comment(&line.text).trim()).0;
        if head.eq_ignore_ascii_case("MACRO") {
            in_macro = true;
            continue;
        }
        if in_macro {
            if head.eq_ignore_ascii_case("ENDM") {
                in_macro = false;
            }
            continue;
        }
        if head.eq_ignore_ascii_case("REPT") || head.eq_ignore_ascii_case("FOR") {
            depth += 1;
        } else if head.eq_ignore_ascii_case("ENDR") {
            depth -= 1;
            if depth == 0 {
                return Ok(index);
            }
        }
    }
    Err(pre_error(
        lines[start].source_line,
        format!("{} block has no matching ENDR", opener.to_ascii_uppercase()),
    ))
}

fn resolve_expansion_id(text: &str, id: Option<usize>) -> String {
    match id {
        Some(id) => text.replace("\\@", &id.to_string()),
        None => text.to_string(),
    }
}

fn invocation(line: &str) -> Option<(String, &str, Option<String>)> {
    let trimmed = line.trim_start();
    let (head, rest) = split_head(trimmed);
    if let Some(label) = head.strip_suffix(':') {
        let (name, tail) = split_head(rest);
        if name.is_empty() {
            None
        } else {
            Some((name.to_string(), tail, Some(label.to_string())))
        }
    } else if head.is_empty() || head.contains('=') {
        None
    } else {
        Some((head.to_string(), rest, None))
    }
}

fn strip_comment(line: &str) -> &str {
    let mut quoted = false;
    for (index, ch) in line.char_indices() {
        if ch == '"' {
            quoted = !quoted;
        } else if ch == ';' && !quoted {
            return &line[..index];
        }
    }
    line
}

fn substitute_symbols(line: &str, symbols: &BTreeMap<String, i64>) -> String {
    if symbols.is_empty() {
        return line.to_string();
    }
    let mut result = String::new();
    let mut chars = line.chars().peekable();
    let mut quoted = false;
    while let Some(ch) = chars.next() {
        if ch == '"' {
            quoted = !quoted;
            result.push(ch);
            continue;
        }
        if !quoted && (ch.is_ascii_alphabetic() || ch == '_') {
            let mut token = String::from(ch);
            while let Some(next) = chars.peek().copied() {
                if next.is_ascii_alphanumeric() || next == '_' || next == '.' {
                    token.push(next);
                    chars.next();
                } else {
                    break;
                }
            }
            if let Some(value) = symbols.get(&token.to_ascii_uppercase()) {
                result.push_str(&value.to_string());
            } else {
                result.push_str(&token);
            }
        } else {
            result.push(ch);
        }
    }
    result
}

fn split_args(text: &str) -> Vec<String> {
    if text.trim().is_empty() {
        return Vec::new();
    }
    let mut args = Vec::new();
    let mut start = 0;
    let mut depth = 0i32;
    let mut quoted = false;
    let chars = text.char_indices().collect::<Vec<_>>();
    for (pos, ch) in &chars {
        match ch {
            '"' => quoted = !quoted,
            '(' | '[' if !quoted => depth += 1,
            ')' | ']' if !quoted => depth -= 1,
            ',' if !quoted && depth == 0 && !text[..*pos].ends_with('\\') => {
                args.push(text[start..*pos].trim().replace("\\,", ","));
                start = *pos + 1;
            }
            _ => {}
        }
    }
    args.push(text[start..].trim().replace("\\,", ","));
    args
}

fn pre_error(line: usize, message: impl Into<String>) -> crate::AssemblyError {
    crate::AssemblyError::Preprocess {
        line,
        message: message.into(),
    }
}

fn evaluate_line(line: &str, symbols: &BTreeMap<String, i64>) -> String {
    let split = line.find(char::is_whitespace).unwrap_or(line.len());
    if split == line.len() {
        return line.to_string();
    }
    let (head, rest) = line.split_at(split);
    let operands = split_args(rest.trim());
    let operands = operands
        .into_iter()
        .map(|operand| {
            let trimmed = operand.trim();
            let (wrapped, expression) = if trimmed.starts_with('(') && trimmed.ends_with(')') {
                (true, &trimmed[1..trimmed.len() - 1])
            } else {
                (false, trimmed)
            };
            let mut parser = ExprParser::new(expression, symbols);
            if let Ok(value) = parser.parse() {
                if wrapped {
                    format!("({value})")
                } else {
                    value.to_string()
                }
            } else {
                trimmed.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("{head} {operands}")
}

struct ExprParser<'a> {
    tokens: Vec<String>,
    index: usize,
    symbols: &'a BTreeMap<String, i64>,
}

impl<'a> ExprParser<'a> {
    fn new(text: &str, symbols: &'a BTreeMap<String, i64>) -> Self {
        Self {
            tokens: tokenize(text),
            index: 0,
            symbols,
        }
    }
    fn parse(&mut self) -> Result<i64, String> {
        let value = self.binary(0)?;
        if self.index != self.tokens.len() {
            return Err(format!(
                "unexpected token `{}` in expression",
                self.tokens[self.index]
            ));
        }
        Ok(value)
    }
    fn binary(&mut self, min: u8) -> Result<i64, String> {
        let mut left = self.unary()?;
        while let Some(op) = self.tokens.get(self.index).cloned() {
            let prec = precedence(&op);
            if prec == 0 || prec < min {
                break;
            }
            self.index += 1;
            let right = self.binary(prec + 1)?;
            left = match op.as_str() {
                "||" => ((left != 0) || (right != 0)) as i64,
                "&&" => ((left != 0) && (right != 0)) as i64,
                "|" => left | right,
                "^" => left ^ right,
                "&" => left & right,
                "==" | "=" => (left == right) as i64,
                "!=" => (left != right) as i64,
                "<" => (left < right) as i64,
                ">" => (left > right) as i64,
                "<=" => (left <= right) as i64,
                ">=" => (left >= right) as i64,
                "+" => left.wrapping_add(right),
                "-" => left.wrapping_sub(right),
                "*" => left.wrapping_mul(right),
                "/" => left
                    .checked_div(right)
                    .ok_or("division by zero".to_string())?,
                "%" => left
                    .checked_rem(right)
                    .ok_or("division by zero".to_string())?,
                "<<" => left.wrapping_shl(right as u32),
                ">>" => left.wrapping_shr(right as u32),
                _ => return Err(format!("unsupported operator `{op}`")),
            };
        }
        Ok(left)
    }
    fn unary(&mut self) -> Result<i64, String> {
        let token = self
            .tokens
            .get(self.index)
            .cloned()
            .ok_or("expected expression".to_string())?;
        self.index += 1;
        match token.as_str() {
            "(" => {
                let value = self.binary(0)?;
                if self.tokens.get(self.index).map(String::as_str) != Some(")") {
                    return Err("expected `)`".into());
                }
                self.index += 1;
                Ok(value)
            }
            "+" => self.unary(),
            "-" => Ok(self.unary()?.wrapping_neg()),
            "~" => Ok(!self.unary()?),
            "!" => Ok((self.unary()? == 0) as i64),
            _ => number(&token)
                .or_else(|| self.symbols.get(&token.to_ascii_uppercase()).copied())
                .ok_or_else(|| format!("unknown symbol or number `{token}`")),
        }
    }
}

fn precedence(op: &str) -> u8 {
    match op {
        "||" => 1,
        "&&" => 2,
        "|" => 3,
        "^" => 4,
        "&" => 5,
        "==" | "=" | "!=" | "<" | ">" | "<=" | ">=" => 6,
        "<<" | ">>" => 7,
        "+" | "-" => 8,
        "*" | "/" | "%" => 9,
        _ => 0,
    }
}

fn tokenize(text: &str) -> Vec<String> {
    let chars = text.chars().collect::<Vec<_>>();
    let mut tokens = Vec::<String>::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }
        if chars[i] == '%'
            && i + 1 < chars.len()
            && matches!(chars[i + 1], '0' | '1')
            && tokens
                .last()
                .is_none_or(|token| precedence(token) > 0 || token == "(")
        {
            let start = i;
            i += 1;
            while i < chars.len() && matches!(chars[i], '0' | '1') {
                i += 1;
            }
            tokens.push(chars[start..i].iter().collect());
            continue;
        }
        if i + 1 < chars.len()
            && matches!(
                (chars[i], chars[i + 1]),
                ('<', '=')
                    | ('>', '=')
                    | ('=', '=')
                    | ('!', '=')
                    | ('&', '&')
                    | ('|', '|')
                    | ('<', '<')
                    | ('>', '>')
            )
        {
            tokens.push(chars[i..=i + 1].iter().collect());
            i += 2;
            continue;
        }
        if "+-*/%()~!&|^<>=".contains(chars[i]) {
            tokens.push(chars[i].to_string());
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && !chars[i].is_whitespace() && !"+-*/%()~!&|^<>=".contains(chars[i])
        {
            i += 1;
        }
        tokens.push(chars[start..i].iter().collect());
    }
    tokens
}

fn number(text: &str) -> Option<i64> {
    let text = text.trim();
    if text.starts_with('$') {
        i64::from_str_radix(&text[1..], 16).ok()
    } else if text.starts_with('%') {
        i64::from_str_radix(&text[1..], 2).ok()
    } else if text.ends_with('h') || text.ends_with('H') {
        i64::from_str_radix(&text[..text.len() - 1], 16).ok()
    } else {
        text.parse().ok()
    }
}
