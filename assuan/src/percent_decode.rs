pub fn percent_decode(x: &str) -> PercentDecoder {
    PercentDecoder(x.chars())
}

pub struct PercentDecoder<'s>(std::str::Chars<'s>);

impl<'s> PercentDecoder<'s> {
    fn decode_next(&mut self) -> Result<Option<char>, MalformedEncoding> {
        match self.0.next() {
            Some('%') => {
                let a = self.0.next().ok_or(MalformedEncoding)?;
                let b = self.0.next().ok_or(MalformedEncoding)?;

                if !a.is_ascii_digit() && !a.is_ascii_uppercase() {
                    return Err(MalformedEncoding);
                }
                if !b.is_ascii_digit() && !b.is_ascii_uppercase() {
                    return Err(MalformedEncoding);
                }

                decode_one_char(a, b).map(Some)
            }
            Some(x) => Ok(Some(x)),
            None => Ok(None),
        }
    }
}

impl<'s> Iterator for PercentDecoder<'s> {
    type Item = Result<char, MalformedEncoding>;

    fn next(&mut self) -> Option<Self::Item> {
        self.decode_next().transpose()
    }
}

pub fn decode_one_char(a: char, b: char) -> Result<char, MalformedEncoding> {
    let a = a.to_digit(16).ok_or(MalformedEncoding)?;
    let b = b.to_digit(16).ok_or(MalformedEncoding)?;

    char::from_u32(a * 0x10 + b).ok_or(MalformedEncoding)
}

#[derive(Debug)]
pub struct MalformedEncoding;

#[cfg(test)]
mod test {
    use super::percent_decode;

    #[test]
    fn test_cases() {
        let cases: &[(&str, &str)] = &[("abcdef", "abcdef"), ("newline%0A", "newline\n")];

        for (input, output) in cases {
            println!("Input: {input}");
            let actual = percent_decode(input)
                .collect::<Result<String, _>>()
                .unwrap();
            assert_eq!(actual, *output);
        }
    }

    #[test]
    fn invalid_encodings() {
        let cases: &[&str] = &["%", "ab%A", "ab%0a", "%FG"];

        for input in cases {
            println!("Input: {input}");
            percent_decode(input)
                .collect::<Result<String, _>>()
                .unwrap_err();
        }
    }
}
