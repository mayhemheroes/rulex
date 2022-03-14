# Emails

[This StackOverflow answer](https://stackoverflow.com/a/201378) contains a massive regular
expression for matching any RFC 5322 compliant email address:

```regexp
(?:[a-z0-9!#$%&'*+/=?^_`{|}~-]+(?:\.[a-z0-9!#$%&'*+/=?^_`{|}~-]+)*|"(?:[\x01-\x08\x0b\x0c\x0e-\x1f\x21\x23-\x5b\x5d-\x7f]|\\[\x01-\x09\x0b\x0c\x0e-\x7f])*")@(?:(?:[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\.)+[a-z0-9](?:[a-z0-9-]*[a-z0-9])?|\[(?:(?:(2(5[0-5]|[0-4][0-9])|1[0-9][0-9]|[1-9]?[0-9]))\.){3}(?:(2(5[0-5]|[0-4][0-9])|1[0-9][0-9]|[1-9]?[0-9])|[a-z0-9-]*[a-z0-9]:(?:[\x01-\x08\x0b\x0c\x0e-\x1f\x21-\x5a\x53-\x7f]|\\[\x01-\x09\x0b\x0c\x0e-\x7f])+)\])
```

If your regex engine supports insiginificant whitespace mode (?x), it can be written like this:

```regexp
(?x)

(?:
  [a-z0-9!#$%&'*+/=?^_`{|}~-]+
  (?: \. [a-z0-9!#$%&'*+/=?^_`{|}~-]+ )*
| "
  (?:
    [\x01-\x08\x0b\x0c\x0e-\x1f\x21\x23-\x5b\x5d-\x7f]
  | \\ [\x01-\x09\x0b\x0c\x0e-\x7f]
  )*
  "
)
@
(?:
  (?: [a-z0-9] (?: [a-z0-9-]* [a-z0-9] )? \. )+
  [a-z0-9]
  (?: [a-z0-9-]* [a-z0-9] )?
| \[
  (?:
    (?: (2 (5 [0-5] | [0-4] [0-9]) | 1 [0-9] [0-9] | [1-9]? [0-9]) )
    \.
  ){3}
  (?:
    (2 (5 [0-5] | [0-4] [0-9]) | 1 [0-9] [0-9] | [1-9]? [0-9])
  | [a-z0-9-]*
    [a-z0-9]
    :
    (?:
      [\x01-\x08\x0b\x0c\x0e-\x1f\x21-\x5a\x53-\x7f]
    | \\ [\x01-\x09\x0b\x0c\x0e-\x7f]
    )+
  )
  \]
)
```

Here's a straightforward translation into rulex:

```regexp
(
  ['a'-'z' '0'-'9' "!#$%&'*+/=?^_`{|}~-"]+ greedy
  ('.' ['a'-'z' '0'-'9' "!#$%&'*+/=?^_`{|}~-"]+ greedy )* greedy
| '"'
  (
    [U+01-U+08 U+0b U+0c U+0e-U+1f U+21 U+23-U+5b U+5d-U+7f]
  | '\' [U+01-U+09 U+0b U+0c U+0e-U+7f]
  )* greedy
  '"'
)
'@'
(
  ( ['a'-'z' '0'-'9'] ( ['a'-'z' '0'-'9' '-']* greedy ['a'-'z' '0'-'9'] )? greedy '.' )+ greedy
  ['a'-'z' '0'-'9']
  ( ['a'-'z' '0'-'9' '-']* greedy ['a'-'z' '0'-'9'] )? greedy
| '['
  (
    :(
      '2' :('5' ['0'-'5'] | ['0'-'4'] ['0'-'9'])
    | '1' ['0'-'9'] ['0'-'9']
    | ['1'-'9']? greedy ['0'-'9']
    )
    '.'
  ){3}
  (
    :(
      '2' :('5' ['0'-'5'] | ['0'-'4'] ['0'-'9'])
    | '1' ['0'-'9'] ['0'-'9']
    | ['1'-'9']? greedy ['0'-'9']
    )
  | ['a'-'z' '0'-'9' '-']* greedy
    ['a'-'z' '0'-'9']
    ':'
    (
      [U+01-U+08 U+0b U+0c U+0e-U+1f U+21-U+5a U+53-U+7f]
    | '\' [U+01-U+09 U+0b U+0c U+0e-U+7f]
    )+
  )
  ']'
)
```

Rulexes this complicated would profit from some features I have planned. They are

- Variables
- Matching a range of decimal numbers

If these features were implemented, we could write the above as follows:

```js
CharBeforeAt = ['a'-'z' '0'-'9' "!#$%&'*+/=?^_`{|}~-"];
QuotedCharBeforeAt = [U+01-U+08 U+0b U+0c U+0e-U+1f U+21 U+23-U+5b U+5d-U+7f];
EscapedCharBeforeAt = '\' [U+01-U+09 U+0b U+0c U+0e-U+7f];

Lower_Digit = ['a'-'z' '0'-'9'];
Lower_Digit_Dash = ['a'-'z' '0'-'9' '-'];

PortDigit = [U+01-U+08 U+0b U+0c U+0e-U+1f U+21-U+5a U+53-U+7f];
EscapedPortChar = '\' [U+01-U+09 U+0b U+0c U+0e-U+7f];


(
  CharBeforeAt+ greedy ('.' CharBeforeAt+ greedy)* greedy
| '"' (QuotedCharBeforeAt | EscapedCharBeforeAt)* greedy '"'
)
'@'
(
  (Lower_Digit (Lower_Digit_Dash* greedy Lower_Digit)? greedy '.')+ greedy
  Lower_Digit
  (Lower_Digit_Dash* greedy Lower_Digit)? greedy
| '['
  (:(range '0'-'255') '.'){3}
  (
    :(range '0'-'255')
  | Lower_Digit_Dash* greedy
    Lower_Digit
    ':'
    (PortDigit | EscapedPortChar)+
  )
  ']'
)
```