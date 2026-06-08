//! Evaluador aritmético determinista (descenso recursivo).
//! Soporta + - * / , paréntesis, decimales y menos unario.
//! Resuelve la debilidad de los LLM con la aritmética exacta.

pub fn eval(expr: &str) -> Result<f64, String> {
    let tokens: Vec<char> = expr.chars().filter(|c| !c.is_whitespace()).collect();
    let mut p = Parser { tokens, pos: 0 };
    let v = p.expr()?;
    if p.pos != p.tokens.len() {
        return Err(format!("token inesperado en posición {}", p.pos));
    }
    Ok(v)
}

struct Parser {
    tokens: Vec<char>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.tokens.get(self.pos).copied()
    }

    fn expr(&mut self) -> Result<f64, String> {
        let mut v = self.term()?;
        while let Some(op) = self.peek() {
            if op == '+' || op == '-' {
                self.pos += 1;
                let rhs = self.term()?;
                v = if op == '+' { v + rhs } else { v - rhs };
            } else {
                break;
            }
        }
        Ok(v)
    }

    fn term(&mut self) -> Result<f64, String> {
        let mut v = self.factor()?;
        while let Some(op) = self.peek() {
            if op == '*' || op == '/' {
                self.pos += 1;
                let rhs = self.factor()?;
                if op == '*' {
                    v *= rhs;
                } else {
                    if rhs == 0.0 {
                        return Err("división por cero".into());
                    }
                    v /= rhs;
                }
            } else {
                break;
            }
        }
        Ok(v)
    }

    fn factor(&mut self) -> Result<f64, String> {
        match self.peek() {
            Some('-') => {
                self.pos += 1;
                Ok(-self.factor()?)
            }
            Some('+') => {
                self.pos += 1;
                self.factor()
            }
            Some('(') => {
                self.pos += 1;
                let v = self.expr()?;
                if self.peek() != Some(')') {
                    return Err("falta ')'".into());
                }
                self.pos += 1;
                Ok(v)
            }
            Some(c) if c.is_ascii_digit() || c == '.' => self.number(),
            other => Err(format!("token inesperado: {other:?}")),
        }
    }

    fn number(&mut self) -> Result<f64, String> {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == '.' {
                self.pos += 1;
            } else {
                break;
            }
        }
        let s: String = self.tokens[start..self.pos].iter().collect();
        s.parse::<f64>()
            .map_err(|_| format!("número inválido: {s}"))
    }
}

#[cfg(test)]
mod tests {
    use super::eval;

    #[test]
    fn fixes_the_arithmetic_bug() {
        // El caso exacto que el LLM falló antes: 47*89-1234 = 2949
        assert_eq!(eval("47*89-1234").unwrap(), 2949.0);
    }

    #[test]
    fn respects_precedence_and_parens() {
        assert_eq!(eval("2+3*4").unwrap(), 14.0);
        assert_eq!(eval("(2+3)*4").unwrap(), 20.0);
        assert_eq!(eval("-5+2").unwrap(), -3.0);
        assert_eq!(eval("10/4").unwrap(), 2.5);
    }

    #[test]
    fn errors_on_div_zero_and_garbage() {
        assert!(eval("1/0").is_err());
        assert!(eval("2++").is_err());
    }
}
