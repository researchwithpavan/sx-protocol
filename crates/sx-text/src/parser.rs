use crate::diagnostics::ParseDiagnostic;
use crate::lexer::{Lexer, Token, TokenKind};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::collections::BTreeMap;
use sx_core::{
    DecimalValue, DeltaDocument, DeltaOp, DeltaOpKind, MessageEnvelope, MoneyValue, ReferenceValue,
    SxColumn, SxError, SxErrorCode, SxPath, SxResult, SxTable, SxTypedArray, SxValue,
};

/// Parses SX text into a logical SX value.
pub fn parse_sx_text(input: &str) -> SxResult<SxValue> {
    parse_sx_text_with_diagnostics(input).map_err(|d| {
        SxError::new(
            SxErrorCode::ParseError,
            format!("{} at {}:{}", d.message, d.line, d.column),
        )
    })
}

/// Parses SX text and returns structured diagnostics.
pub fn parse_sx_text_with_diagnostics(input: &str) -> Result<SxValue, ParseDiagnostic> {
    let tokens = Lexer::new(input)
        .tokenize()
        .map_err(|m| ParseDiagnostic::new(m, 1, 1))?;
    let mut p = Parser { tokens, i: 0 };
    let v = p.parse_root()?;
    p.expect_eof()?;
    Ok(v)
}

struct Parser {
    tokens: Vec<Token>,
    i: usize,
}

impl Parser {
    fn parse_root(&mut self) -> Result<SxValue, ParseDiagnostic> {
        match self.peek_kind() {
            TokenKind::Identifier(id) if id == "message" => self.parse_message(),
            TokenKind::Identifier(id) if id == "table" => self.parse_table(),
            TokenKind::Identifier(id) if id == "delta" => self.parse_delta(),
            TokenKind::Identifier(id) if id == "schema" => self.parse_schema(),
            _ => self.parse_value(),
        }
    }

    fn parse_value(&mut self) -> Result<SxValue, ParseDiagnostic> {
        match self.peek_kind() {
            TokenKind::LBrace => self.parse_object(),
            TokenKind::LBracket => self.parse_array(),
            TokenKind::String(s) => {
                let s = s.clone();
                self.bump();
                Ok(SxValue::String(s))
            }
            TokenKind::Number(n) => {
                let n = n.clone();
                self.bump();
                parse_number(&n).map_err(|m| self.error(m))
            }
            TokenKind::Identifier(id) if id == "null" => {
                self.bump();
                Ok(SxValue::Null)
            }
            TokenKind::Identifier(id) if id == "true" => {
                self.bump();
                Ok(SxValue::Bool(true))
            }
            TokenKind::Identifier(id) if id == "false" => {
                self.bump();
                Ok(SxValue::Bool(false))
            }
            TokenKind::Identifier(id) => {
                let id = id.clone();
                self.bump();
                match self.peek_kind() {
                    TokenKind::LParen => self.parse_typed_literal(&id),
                    TokenKind::LBracket if is_typed_array_ident(&id) => self.parse_typed_array(&id),
                    _ => Ok(SxValue::String(id)),
                }
            }
            other => Err(self.error(format!("unexpected token {other:?}"))),
        }
    }

    fn parse_object(&mut self) -> Result<SxValue, ParseDiagnostic> {
        self.expect(TokenKind::LBrace)?;
        let mut map = BTreeMap::new();
        loop {
            if matches!(self.peek_kind(), TokenKind::RBrace) {
                self.bump();
                break;
            }
            let key = self.parse_key()?;
            self.expect(TokenKind::Colon)?;
            let value = self.parse_value()?;
            if map.insert(key.clone(), value).is_some() {
                return Err(self.error(format!("duplicate key '{key}'")));
            }
            if matches!(self.peek_kind(), TokenKind::Comma) {
                self.bump();
                if matches!(self.peek_kind(), TokenKind::RBrace) {
                    self.bump();
                    break;
                }
            } else if matches!(self.peek_kind(), TokenKind::RBrace) {
                self.bump();
                break;
            } else {
                return Err(self.error("expected ',' or '}'"));
            }
        }
        Ok(SxValue::Object(map))
    }

    fn parse_array(&mut self) -> Result<SxValue, ParseDiagnostic> {
        self.expect(TokenKind::LBracket)?;
        let mut out = Vec::new();
        loop {
            if matches!(self.peek_kind(), TokenKind::RBracket) {
                self.bump();
                break;
            }
            out.push(self.parse_value()?);
            if matches!(self.peek_kind(), TokenKind::Comma) {
                self.bump();
                if matches!(self.peek_kind(), TokenKind::RBracket) {
                    self.bump();
                    break;
                }
            } else if matches!(self.peek_kind(), TokenKind::RBracket) {
                self.bump();
                break;
            } else {
                return Err(self.error("expected ',' or ']'"));
            }
        }
        Ok(SxValue::Array(out))
    }

    fn parse_typed_array(&mut self, ident: &str) -> Result<SxValue, ParseDiagnostic> {
        self.expect(TokenKind::LBracket)?;
        match ident {
            "u8" => {
                let mut v = Vec::new();
                loop {
                    if matches!(self.peek_kind(), TokenKind::RBracket) {
                        self.bump();
                        break;
                    }
                    let n = self.parse_number_lit()?;
                    v.push(n as u8);
                    self.consume_arr_sep()?;
                }
                Ok(SxValue::TypedArray(SxTypedArray::U8(v)))
            }
            "i32" => {
                let mut v = Vec::new();
                loop {
                    if matches!(self.peek_kind(), TokenKind::RBracket) {
                        self.bump();
                        break;
                    }
                    let n = self.parse_number_lit()?;
                    v.push(n as i32);
                    self.consume_arr_sep()?;
                }
                Ok(SxValue::TypedArray(SxTypedArray::I32(v)))
            }
            "f32" => {
                let mut v = Vec::new();
                loop {
                    if matches!(self.peek_kind(), TokenKind::RBracket) {
                        self.bump();
                        break;
                    }
                    v.push(self.parse_float_lit()? as f32);
                    self.consume_arr_sep()?;
                }
                Ok(SxValue::TypedArray(SxTypedArray::F32(v)))
            }
            "f64" => {
                let mut v = Vec::new();
                loop {
                    if matches!(self.peek_kind(), TokenKind::RBracket) {
                        self.bump();
                        break;
                    }
                    v.push(self.parse_float_lit()?);
                    self.consume_arr_sep()?;
                }
                Ok(SxValue::TypedArray(SxTypedArray::F64(v)))
            }
            "bool" => {
                let mut v = Vec::new();
                loop {
                    if matches!(self.peek_kind(), TokenKind::RBracket) {
                        self.bump();
                        break;
                    }
                    match self.peek_kind() {
                        TokenKind::Identifier(id) if id == "true" => {
                            self.bump();
                            v.push(true);
                        }
                        TokenKind::Identifier(id) if id == "false" => {
                            self.bump();
                            v.push(false);
                        }
                        _ => return Err(self.error("expected true/false in bool[]")),
                    }
                    self.consume_arr_sep()?;
                }
                Ok(SxValue::TypedArray(SxTypedArray::Bool(v)))
            }
            _ => Err(self.error("unsupported typed array")),
        }
    }

    fn consume_arr_sep(&mut self) -> Result<(), ParseDiagnostic> {
        if matches!(self.peek_kind(), TokenKind::Comma) {
            self.bump();
            Ok(())
        } else if matches!(self.peek_kind(), TokenKind::RBracket) {
            Ok(())
        } else {
            Err(self.error("expected ',' or ']'"))
        }
    }

    fn parse_typed_literal(&mut self, ident: &str) -> Result<SxValue, ParseDiagnostic> {
        self.expect(TokenKind::LParen)?;
        let val = match ident {
            "uuid" => {
                let s = self.parse_string()?;
                let u = uuid::Uuid::parse_str(&s)
                    .map_err(|e| self.error(format!("invalid uuid: {e}")))?;
                SxValue::Uuid(*u.as_bytes())
            }
            "timestamp" => SxValue::Timestamp(self.parse_string()?),
            "date" => SxValue::Date(self.parse_string()?),
            "duration" => SxValue::Duration(self.parse_string()?),
            "url" => SxValue::Url(self.parse_string()?),
            "email" => SxValue::Email(self.parse_string()?),
            "bytes" => {
                let s = self.parse_string()?;
                let payload = s.strip_prefix("base64:").unwrap_or(&s);
                let b = STANDARD
                    .decode(payload)
                    .map_err(|e| self.error(format!("invalid base64: {e}")))?;
                SxValue::Bytes(b)
            }
            "decimal" => {
                let text = self.parse_string()?;
                let mut scale = 0u32;
                if matches!(self.peek_kind(), TokenKind::Comma) {
                    self.bump();
                    let key = self.parse_key()?;
                    if key != "scale" {
                        return Err(self.error("expected scale"));
                    }
                    self.expect(TokenKind::Colon)?;
                    scale = self.parse_number_lit()? as u32;
                }
                let scaled = parse_scaled_decimal(&text, scale)?;
                SxValue::Decimal(DecimalValue { scaled, scale })
            }
            "money" => {
                let currency = self.parse_string()?;
                self.expect(TokenKind::Comma)?;
                let scaled = self.parse_number_lit()?;
                self.expect(TokenKind::Comma)?;
                let key = self.parse_key()?;
                if key != "scale" {
                    return Err(self.error("expected scale"));
                }
                self.expect(TokenKind::Colon)?;
                let scale = self.parse_number_lit()? as u32;
                SxValue::Money(MoneyValue {
                    currency,
                    scaled,
                    scale,
                })
            }
            "ref" => SxValue::Reference(ReferenceValue {
                target: self.parse_string()?,
            }),
            _ => return Err(self.error(format!("unsupported typed literal '{ident}'"))),
        };
        self.expect(TokenKind::RParen)?;
        Ok(val)
    }

    fn parse_table(&mut self) -> Result<SxValue, ParseDiagnostic> {
        self.expect_ident("table")?;
        if matches!(self.peek_kind(), TokenKind::Identifier(_)) {
            self.bump();
        }
        self.expect(TokenKind::LBrace)?;
        let mut cols = BTreeMap::new();
        loop {
            if matches!(self.peek_kind(), TokenKind::RBrace) {
                self.bump();
                break;
            }
            let name = self.parse_key()?;
            self.expect(TokenKind::Colon)?;
            let col_value = self.parse_value()?;
            let col = match col_value {
                SxValue::TypedArray(t) => SxColumn::Typed(t),
                SxValue::Array(v) => SxColumn::Values(v),
                _ => return Err(self.error("table column must be array or typed array")),
            };
            cols.insert(name, col);
            if matches!(self.peek_kind(), TokenKind::Comma) {
                self.bump();
                if matches!(self.peek_kind(), TokenKind::RBrace) {
                    self.bump();
                    break;
                }
            } else if matches!(self.peek_kind(), TokenKind::RBrace) {
                self.bump();
                break;
            } else {
                return Err(self.error("expected ',' or '}'"));
            }
        }
        Ok(SxValue::Table(SxTable { columns: cols }))
    }

    fn parse_message(&mut self) -> Result<SxValue, ParseDiagnostic> {
        self.expect_ident("message")?;
        if matches!(self.peek_kind(), TokenKind::Identifier(_)) {
            self.bump();
        }
        let payload = self.parse_object()?;
        let msg = match payload {
            SxValue::Object(fields) => MessageEnvelope {
                sx_version: 1,
                message_id: None,
                message_type: None,
                schema: None,
                timestamp: None,
                logical_hash: None,
                fields: fields.clone(),
                payload: Some(Box::new(SxValue::Object(fields))),
            },
            _ => return Err(self.error("message body must be object")),
        };
        Ok(SxValue::Message(msg))
    }

    fn parse_delta(&mut self) -> Result<SxValue, ParseDiagnostic> {
        self.expect_ident("delta")?;
        let mut from_hash = None;
        if self.match_ident("from") {
            self.expect_ident("hash")?;
            self.expect(TokenKind::LParen)?;
            from_hash = Some(self.parse_string()?);
            self.expect(TokenKind::RParen)?;
        }
        self.expect(TokenKind::LBrace)?;
        let mut ops = Vec::new();
        loop {
            if matches!(self.peek_kind(), TokenKind::RBrace) {
                self.bump();
                break;
            }
            let op_name = self.parse_ident()?;
            let kind = parse_delta_kind(&op_name).ok_or_else(|| self.error("invalid delta op"))?;
            let path_tok = match self.peek_kind() {
                TokenKind::Path(p) => {
                    let p = p.clone();
                    self.bump();
                    p
                }
                _ => return Err(self.error("delta op requires path")),
            };
            let path = SxPath::parse(&path_tok).map_err(|e| self.error(e.message))?;
            let mut value = None;
            let mut from = None;
            let mut index = None;
            match kind {
                DeltaOpKind::Remove | DeltaOpKind::Clear => {}
                DeltaOpKind::Increment | DeltaOpKind::Decrement => {
                    self.expect_ident("by")?;
                    value = Some(self.parse_value()?);
                }
                DeltaOpKind::Insert => {
                    self.expect_ident("at")?;
                    index = Some(self.parse_number_lit()? as usize);
                    value = Some(self.parse_value()?);
                }
                DeltaOpKind::Move | DeltaOpKind::Copy => {
                    self.expect_ident("from")?;
                    let p = match self.peek_kind() {
                        TokenKind::Path(p) => {
                            let p = p.clone();
                            self.bump();
                            p
                        }
                        _ => return Err(self.error("expected source path")),
                    };
                    from = Some(SxPath::parse(&p).map_err(|e| self.error(e.message))?);
                }
                _ => {
                    if matches!(self.peek_kind(), TokenKind::Eq) {
                        self.bump();
                    }
                    value = Some(self.parse_value()?);
                }
            }
            ops.push(DeltaOp {
                kind,
                path,
                value,
                from,
                index,
            });
            if matches!(self.peek_kind(), TokenKind::Comma) {
                self.bump();
            }
        }
        Ok(SxValue::Delta(DeltaDocument { from_hash, ops }))
    }

    fn parse_schema(&mut self) -> Result<SxValue, ParseDiagnostic> {
        self.expect_ident("schema")?;
        let name = self.parse_ident()?;
        let mut version = 1i64;
        if let TokenKind::Identifier(v) = self.peek_kind() {
            if let Some(num) = v.strip_prefix('v') {
                version = num
                    .parse::<i64>()
                    .map_err(|_| self.error("invalid schema version"))?;
                self.bump();
            }
        }
        self.expect(TokenKind::LBrace)?;
        let mut fields = Vec::new();
        loop {
            if matches!(self.peek_kind(), TokenKind::RBrace) {
                self.bump();
                break;
            }
            let mut hints = Vec::new();
            while matches!(self.peek_kind(), TokenKind::At) {
                self.bump();
                hints.push(self.parse_ident()?);
            }
            let field_id = match self.peek_kind() {
                TokenKind::Number(n) => {
                    let id = n
                        .parse::<i64>()
                        .map_err(|_| self.error("invalid schema field id"))?;
                    self.bump();
                    id
                }
                _ => (fields.len() + 1) as i64,
            };
            let fname = self.parse_key()?;
            let mut optional = false;
            if matches!(self.peek_kind(), TokenKind::Question) {
                self.bump();
                optional = true;
            }
            self.expect(TokenKind::Colon)?;
            let field_ty = match self.peek_kind() {
                TokenKind::Identifier(id) => {
                    let id = id.clone();
                    self.bump();
                    id
                }
                _ => return Err(self.error("schema field type expected")),
            };
            let mut fobj = BTreeMap::new();
            fobj.insert("id".to_string(), SxValue::I64(field_id));
            fobj.insert("name".to_string(), SxValue::String(fname));
            fobj.insert("type".to_string(), SxValue::String(field_ty));
            fobj.insert("optional".to_string(), SxValue::Bool(optional));
            fobj.insert(
                "hints".to_string(),
                SxValue::Array(hints.into_iter().map(SxValue::String).collect()),
            );
            fields.push(SxValue::Object(fobj));
            if matches!(self.peek_kind(), TokenKind::Comma) {
                self.bump();
            }
        }
        let mut obj = BTreeMap::new();
        obj.insert("$schema_name".to_string(), SxValue::String(name));
        obj.insert("$schema_version".to_string(), SxValue::I64(version));
        obj.insert("$fields".to_string(), SxValue::Array(fields));
        Ok(SxValue::Object(obj))
    }

    fn parse_key(&mut self) -> Result<String, ParseDiagnostic> {
        match self.peek_kind() {
            TokenKind::Identifier(s) | TokenKind::String(s) => {
                let s = s.clone();
                self.bump();
                Ok(s)
            }
            _ => Err(self.error("expected object key")),
        }
    }

    fn parse_ident(&mut self) -> Result<String, ParseDiagnostic> {
        match self.peek_kind() {
            TokenKind::Identifier(s) => {
                let s = s.clone();
                self.bump();
                Ok(s)
            }
            _ => Err(self.error("expected identifier")),
        }
    }

    fn parse_string(&mut self) -> Result<String, ParseDiagnostic> {
        match self.peek_kind() {
            TokenKind::String(s) => {
                let s = s.clone();
                self.bump();
                Ok(s)
            }
            _ => Err(self.error("expected string")),
        }
    }

    fn parse_number_lit(&mut self) -> Result<i64, ParseDiagnostic> {
        match self.peek_kind() {
            TokenKind::Number(n) => {
                let n = n.clone();
                self.bump();
                n.parse::<i64>()
                    .map_err(|e| self.error(format!("invalid integer: {e}")))
            }
            _ => Err(self.error("expected integer")),
        }
    }

    fn parse_float_lit(&mut self) -> Result<f64, ParseDiagnostic> {
        match self.peek_kind() {
            TokenKind::Number(n) => {
                let n = n.clone();
                self.bump();
                n.parse::<f64>()
                    .map_err(|e| self.error(format!("invalid float: {e}")))
            }
            _ => Err(self.error("expected float")),
        }
    }

    fn expect_ident(&mut self, expected: &str) -> Result<(), ParseDiagnostic> {
        match self.peek_kind() {
            TokenKind::Identifier(id) if id == expected => {
                self.bump();
                Ok(())
            }
            _ => Err(self.error(format!("expected identifier '{expected}'"))),
        }
    }

    fn match_ident(&mut self, expected: &str) -> bool {
        if let TokenKind::Identifier(id) = self.peek_kind() {
            if id == expected {
                self.bump();
                return true;
            }
        }
        false
    }

    fn expect(&mut self, kind: TokenKind) -> Result<(), ParseDiagnostic> {
        if std::mem::discriminant(self.peek_kind()) == std::mem::discriminant(&kind) {
            self.bump();
            Ok(())
        } else {
            Err(self.error(format!("expected {:?}", kind)))
        }
    }

    fn expect_eof(&self) -> Result<(), ParseDiagnostic> {
        if matches!(self.peek_kind(), TokenKind::Eof) {
            Ok(())
        } else {
            Err(self.error("unexpected trailing tokens"))
        }
    }

    fn peek_kind(&self) -> &TokenKind {
        self.tokens
            .get(self.i)
            .map(|t| &t.kind)
            .unwrap_or(&TokenKind::Eof)
    }

    fn bump(&mut self) {
        self.i += 1;
    }

    fn error(&self, message: impl Into<String>) -> ParseDiagnostic {
        let t = self.tokens.get(self.i).or_else(|| self.tokens.last());
        if let Some(t) = t {
            ParseDiagnostic::new(message, t.line, t.column)
        } else {
            ParseDiagnostic::new(message, 1, 1)
        }
    }
}

fn is_typed_array_ident(ident: &str) -> bool {
    matches!(ident, "u8" | "i32" | "f32" | "f64" | "bool")
}

fn parse_delta_kind(s: &str) -> Option<DeltaOpKind> {
    Some(match s {
        "set" => DeltaOpKind::Set,
        "replace" => DeltaOpKind::Replace,
        "remove" => DeltaOpKind::Remove,
        "append" => DeltaOpKind::Append,
        "prepend" => DeltaOpKind::Prepend,
        "insert" => DeltaOpKind::Insert,
        "increment" => DeltaOpKind::Increment,
        "decrement" => DeltaOpKind::Decrement,
        "merge" => DeltaOpKind::Merge,
        "move" => DeltaOpKind::Move,
        "copy" => DeltaOpKind::Copy,
        "clear" => DeltaOpKind::Clear,
        _ => return None,
    })
}

fn parse_number(text: &str) -> Result<SxValue, String> {
    if text.contains('.') || text.contains('e') || text.contains('E') {
        text.parse::<f64>()
            .map(SxValue::F64)
            .map_err(|e| format!("invalid float: {e}"))
    } else {
        text.parse::<i64>()
            .map(SxValue::I64)
            .map_err(|e| format!("invalid int: {e}"))
    }
}

fn parse_scaled_decimal(input: &str, scale: u32) -> Result<i128, ParseDiagnostic> {
    let s = input.trim();
    if let Some((int_part, frac_part)) = s.split_once('.') {
        let sign = if int_part.starts_with('-') {
            -1i128
        } else {
            1i128
        };
        let abs_int = int_part
            .trim_start_matches('-')
            .parse::<i128>()
            .map_err(|_| ParseDiagnostic::new("invalid decimal integer part", 1, 1))?;
        let mut frac = frac_part.to_string();
        if frac.len() > scale as usize {
            frac.truncate(scale as usize);
        }
        while frac.len() < scale as usize {
            frac.push('0');
        }
        let frac_val = if frac.is_empty() {
            0
        } else {
            frac.parse::<i128>()
                .map_err(|_| ParseDiagnostic::new("invalid decimal fraction", 1, 1))?
        };
        Ok(sign * (abs_int * 10i128.pow(scale) + frac_val))
    } else {
        let base = s
            .parse::<i128>()
            .map_err(|_| ParseDiagnostic::new("invalid decimal value", 1, 1))?;
        Ok(base * 10i128.pow(scale))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_object_and_typed_array() {
        let input = r#"{ user: { id: uuid("018f4b5e-7a24-7c8a-b28d-3f951a1b7f13"), active: true }, temps: f32[1.2, 3.4,], }"#;
        let v = parse_sx_text(input).unwrap();
        let SxValue::Object(m) = v else {
            panic!("expected object")
        };
        assert!(m.contains_key("user"));
        assert!(m.contains_key("temps"));
    }

    #[test]
    fn parse_delta() {
        let input = r#"delta from hash("abc") { set /a = 1, increment /n by 2, remove /x }"#;
        let v = parse_sx_text(input).unwrap();
        let SxValue::Delta(d) = v else {
            panic!("expected delta")
        };
        assert_eq!(d.ops.len(), 3);
    }
}
