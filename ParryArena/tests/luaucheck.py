"""Small Luau-subset checker: syntax + scope (undefined globals, use before local definition).

Usage: python luaucheck.py file.lua [more.lua ...]
"""
import re
import sys

KEYWORDS = {
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "if", "in",
    "local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while", "continue",
}

KNOWN_GLOBALS = {
    "game", "self", "handles", "Vector3", "Vector2", "Color3", "UDim", "UDim2", "Rect", "Instance",
    "task", "rotation", "geometry", "DynamicMesh", "print", "warn", "tick", "typeof", "type", "pcall",
    "xpcall", "error", "ipairs", "pairs", "next", "select", "tostring", "tonumber", "math", "string",
    "table", "bit32", "coroutine", "utf8", "buffer", "setmetatable", "getmetatable", "rawget",
    "rawset", "rawequal", "rawlen", "assert", "EventTableDump", "vector", "Update", "PhysicsUpdate",
    "os",
}

ALLOWED_GLOBAL_ASSIGN = {"Update", "PhysicsUpdate"}


class Tok:
    __slots__ = ("kind", "val", "line")

    def __init__(self, kind, val, line):
        self.kind, self.val, self.line = kind, val, line

    def __repr__(self):
        return f"{self.kind}:{self.val}@{self.line}"


def tokenize(src):
    toks = []
    i, n, line = 0, len(src), 1
    ops3 = ("...", "//=", "..=")
    ops2 = ("==", "~=", "<=", ">=", "..", "::", "+=", "-=", "*=", "/=", "%=", "^=", "->", "//")
    while i < n:
        c = src[i]
        if c == "\n":
            line += 1
            i += 1
            continue
        if c in " \t\r":
            i += 1
            continue
        if src.startswith("--", i):
            m = re.match(r"--\[(=*)\[", src[i:])
            if m:
                close = "]" + m.group(1) + "]"
                j = src.find(close, i + m.end())
                if j < 0:
                    raise SyntaxError(f"line {line}: unterminated long comment")
                line += src.count("\n", i, j)
                i = j + len(close)
            else:
                j = src.find("\n", i)
                i = n if j < 0 else j
            continue
        m = re.match(r"\[(=*)\[", src[i:])
        if m:
            close = "]" + m.group(1) + "]"
            j = src.find(close, i + m.end())
            if j < 0:
                raise SyntaxError(f"line {line}: unterminated long string")
            toks.append(Tok("str", src[i + m.end():j], line))
            line += src.count("\n", i, j)
            i = j + len(close)
            continue
        if c in "\"'`":
            q = c
            j = i + 1
            buf = []
            while j < n and src[j] != q:
                if src[j] == "\\":
                    buf.append(src[j:j + 2])
                    j += 2
                    continue
                if src[j] == "\n":
                    raise SyntaxError(f"line {line}: newline in string")
                buf.append(src[j])
                j += 1
            if j >= n:
                raise SyntaxError(f"line {line}: unterminated string")
            toks.append(Tok("str", "".join(buf), line))
            i = j + 1
            continue
        m = re.match(r"0[xX][0-9a-fA-F_]+|0[bB][01_]+|(\d[\d_]*\.?[\d_]*([eE][+-]?\d+)?|\.\d+([eE][+-]?\d+)?)", src[i:])
        if m and (c.isdigit() or (c == "." and i + 1 < n and src[i + 1].isdigit())):
            toks.append(Tok("num", m.group(0), line))
            i += m.end()
            continue
        m = re.match(r"[A-Za-z_][A-Za-z0-9_]*", src[i:])
        if m:
            w = m.group(0)
            toks.append(Tok("kw" if w in KEYWORDS else "name", w, line))
            i += m.end()
            continue
        for op in ops3 + ops2:
            if src.startswith(op, i):
                toks.append(Tok("op", op, line))
                i += len(op)
                break
        else:
            if c in "+-*/%^#&~|<>=(){}[];:,.@":
                toks.append(Tok("op", c, line))
                i += 1
            else:
                raise SyntaxError(f"line {line}: unexpected character {c!r}")
    toks.append(Tok("eof", "", line))
    return toks


BINOPS = {"+", "-", "*", "/", "//", "%", "^", "..", "==", "~=", "<", "<=", ">", ">=", "and", "or"}
COMPOUND = {"+=", "-=", "*=", "/=", "//=", "%=", "^=", "..="}


class Checker:
    def __init__(self, toks, fname):
        self.t = toks
        self.i = 0
        self.f = fname
        self.scopes = [{}]
        self.problems = []
        self.global_uses = {}

    # --- helpers
    def peek(self, k=0):
        return self.t[self.i + k]

    def next(self):
        tok = self.t[self.i]
        self.i += 1
        return tok

    def check(self, val):
        tok = self.peek()
        return tok.val == val and tok.kind in ("op", "kw")

    def accept(self, val):
        if self.check(val):
            return self.next()
        return None

    def expect(self, val):
        tok = self.next()
        if tok.val != val or tok.kind not in ("op", "kw"):
            raise SyntaxError(f"{self.f}:{tok.line}: expected '{val}' near '{tok.val}'")
        return tok

    def name(self):
        tok = self.next()
        if tok.kind != "name":
            raise SyntaxError(f"{self.f}:{tok.line}: expected a name near '{tok.val}'")
        return tok

    def declare(self, name, line):
        self.scopes[-1][name] = line

    def resolve(self, tok, assigning=False):
        for scope in reversed(self.scopes):
            if tok.val in scope:
                return True
        if assigning and tok.val not in ALLOWED_GLOBAL_ASSIGN:
            self.problems.append(f"{self.f}:{tok.line}: assignment to undeclared global '{tok.val}'")
        elif not assigning and tok.val not in KNOWN_GLOBALS:
            self.problems.append(f"{self.f}:{tok.line}: use of undeclared name '{tok.val}' (not a local in scope here)")
        return False

    def push(self):
        self.scopes.append({})

    def pop(self):
        self.scopes.pop()

    # --- grammar
    def chunk(self):
        self.block()
        if self.peek().kind != "eof":
            tok = self.peek()
            raise SyntaxError(f"{self.f}:{tok.line}: unexpected '{tok.val}'")

    def block_end(self):
        tok = self.peek()
        return tok.kind == "eof" or (tok.kind == "kw" and tok.val in ("end", "else", "elseif", "until"))

    def block(self):
        while not self.block_end():
            if self.check("return"):
                self.next()
                if not self.block_end() and not self.check(";"):
                    self.explist()
                self.accept(";")
                if not self.block_end():
                    tok = self.peek()
                    raise SyntaxError(f"{self.f}:{tok.line}: code after return near '{tok.val}'")
                return
            self.statement()
            self.accept(";")

    def statement(self):
        tok = self.peek()
        v = tok.val
        if tok.kind == "kw":
            if v == "local":
                self.next()
                if self.accept("function"):
                    nm = self.name()
                    self.declare(nm.val, nm.line)
                    self.funcbody()
                    return
                names = [self.name()]
                self.skip_attrib_type()
                while self.accept(","):
                    names.append(self.name())
                    self.skip_attrib_type()
                if self.accept("="):
                    self.explist()
                for nm in names:
                    self.declare(nm.val, nm.line)
                return
            if v == "function":
                self.next()
                first = self.name()
                is_method_path = False
                while self.check(".") or self.check(":"):
                    self.next()
                    self.name()
                    is_method_path = True
                if is_method_path:
                    self.resolve(first)
                else:
                    self.resolve(first, assigning=True)
                self.funcbody(method=True)
                return
            if v == "if":
                self.next()
                self.exp()
                self.expect("then")
                self.push(); self.block(); self.pop()
                while self.accept("elseif"):
                    self.exp()
                    self.expect("then")
                    self.push(); self.block(); self.pop()
                if self.accept("else"):
                    self.push(); self.block(); self.pop()
                self.expect("end")
                return
            if v == "while":
                self.next()
                self.exp()
                self.expect("do")
                self.push(); self.block(); self.pop()
                self.expect("end")
                return
            if v == "for":
                self.next()
                first = self.name()
                if self.accept("="):
                    self.exp(); self.expect(","); self.exp()
                    if self.accept(","):
                        self.exp()
                    names = [first]
                else:
                    names = [first]
                    while self.accept(","):
                        names.append(self.name())
                    self.expect("in")
                    self.explist()
                self.expect("do")
                self.push()
                for nm in names:
                    self.declare(nm.val, nm.line)
                self.block()
                self.pop()
                self.expect("end")
                return
            if v == "repeat":
                self.next()
                self.push(); self.block()
                self.expect("until")
                self.exp()
                self.pop()
                return
            if v == "do":
                self.next()
                self.push(); self.block(); self.pop()
                self.expect("end")
                return
            if v in ("break", "continue"):
                self.next()
                return
        # expression statement
        targets = [self.suffixedexp(stat=True)]
        if self.check("=") or self.check(","):
            while self.accept(","):
                targets.append(self.suffixedexp(stat=True))
            self.expect("=")
            self.explist()
            for kind, tokk in targets:
                if kind == "call":
                    raise SyntaxError(f"{self.f}:{tokk.line}: cannot assign to a call")
                if kind == "name":
                    self.resolve(tokk, assigning=True)
            return
        if self.peek().kind == "op" and self.peek().val in COMPOUND:
            self.next()
            self.exp()
            kind, tokk = targets[0]
            if kind == "name":
                self.resolve(tokk)
            return
        kind, tokk = targets[0]
        if kind != "call":
            raise SyntaxError(f"{self.f}:{tokk.line}: syntax error near '{self.peek().val}' (expression is not a statement)")

    def skip_attrib_type(self):
        if self.accept(":"):
            self.type_expr()

    def type_expr(self):
        # loose: consume a simple type (names, dots, ?, |, &, {}, ())
        depth = 0
        while True:
            tok = self.peek()
            if tok.kind == "eof":
                return
            if tok.val in ("(", "{", "<") and tok.kind == "op":
                depth += 1
            elif tok.val in (")", "}", ">") and tok.kind == "op":
                if depth == 0:
                    return
                depth -= 1
            elif depth == 0 and (tok.val in ("=", ",", ")") or tok.kind == "kw"):
                return
            self.next()

    def funcbody(self, method=False):
        self.expect("(")
        self.push()
        if method:
            self.declare("self", 0)
        params = []
        if not self.check(")"):
            while True:
                if self.accept("..."):
                    self.declare("...", 0)
                    break
                nm = self.name()
                params.append(nm)
                self.skip_attrib_type()
                if not self.accept(","):
                    break
        self.expect(")")
        if self.accept(":"):
            self.type_expr()
        for nm in params:
            self.declare(nm.val, nm.line)
        self.block()
        self.expect("end")
        self.pop()

    def explist(self):
        self.exp()
        while self.accept(","):
            self.exp()

    def exp(self):
        tok = self.peek()
        if (tok.kind == "kw" and tok.val == "not") or (tok.kind == "op" and tok.val in ("-", "#")):
            self.next()
            self.exp_unary_tail()
        else:
            self.simpleexp()
        while True:
            tok = self.peek()
            if tok.val in BINOPS and tok.kind in ("op", "kw"):
                self.next()
                t2 = self.peek()
                if (t2.kind == "kw" and t2.val == "not") or (t2.kind == "op" and t2.val in ("-", "#")):
                    self.next()
                    self.exp_unary_tail()
                else:
                    self.simpleexp()
            else:
                return

    def exp_unary_tail(self):
        tok = self.peek()
        if (tok.kind == "kw" and tok.val == "not") or (tok.kind == "op" and tok.val in ("-", "#")):
            self.next()
            self.exp_unary_tail()
        else:
            self.simpleexp()

    def simpleexp(self):
        tok = self.peek()
        if tok.kind in ("num", "str"):
            self.next()
            return
        if tok.kind == "kw" and tok.val in ("nil", "true", "false"):
            self.next()
            return
        if tok.kind == "op" and tok.val == "...":
            self.next()
            return
        if tok.kind == "kw" and tok.val == "function":
            self.next()
            self.funcbody()
            return
        if tok.kind == "kw" and tok.val == "if":
            self.next()
            self.exp(); self.expect("then"); self.exp()
            while self.accept("elseif"):
                self.exp(); self.expect("then"); self.exp()
            self.expect("else")
            self.exp()
            return
        if tok.kind == "op" and tok.val == "{":
            self.table()
            return
        self.suffixedexp()

    def table(self):
        self.expect("{")
        while not self.check("}"):
            if self.accept("["):
                self.exp(); self.expect("]"); self.expect("="); self.exp()
            elif self.peek().kind == "name" and self.peek(1).val == "=" and self.peek(1).kind == "op":
                self.next(); self.next(); self.exp()
            else:
                self.exp()
            if not (self.accept(",") or self.accept(";")):
                break
        self.expect("}")

    def suffixedexp(self, stat=False):
        tok = self.peek()
        kind = None
        first = tok
        if tok.kind == "name":
            self.next()
            kind = "name"
            if not stat:
                self.resolve(tok)
            else:
                # resolved later if it turns out to be a plain assignment target
                nxt = self.peek()
                plain_target = nxt.kind == "op" and nxt.val in ("=", ",")
                if not plain_target:
                    self.resolve(tok)
        elif tok.kind == "op" and tok.val == "(":
            self.next()
            self.exp()
            self.expect(")")
            kind = "paren"
        else:
            raise SyntaxError(f"{self.f}:{tok.line}: unexpected '{tok.val}'")
        while True:
            tok = self.peek()
            if tok.kind == "op" and tok.val == ".":
                self.next(); self.name(); kind = "index"
            elif tok.kind == "op" and tok.val == "[":
                self.next(); self.exp(); self.expect("]"); kind = "index"
            elif tok.kind == "op" and tok.val == ":":
                self.next(); self.name(); self.args(); kind = "call"
            elif (tok.kind == "op" and tok.val in ("(", "{")) or tok.kind == "str":
                if tok.kind == "op" and tok.val == "(" and tok.line != self.t[self.i - 1].line:
                    # Lua's ambiguous-syntax rule: a call cannot start on a new line
                    raise SyntaxError(f"{self.f}:{tok.line}: ambiguous call (parenthesis on a new line)")
                self.args(); kind = "call"
            else:
                break
        return kind, first

    def args(self):
        tok = self.peek()
        if tok.kind == "str":
            self.next()
            return
        if tok.val == "{":
            self.table()
            return
        self.expect("(")
        if not self.check(")"):
            self.explist()
        self.expect(")")


def check_file(path):
    src = open(path, encoding="utf-8").read()
    toks = tokenize(src)
    c = Checker(toks, path.split("/")[-1])
    c.chunk()
    return c.problems


if __name__ == "__main__":
    bad = 0
    for p in sys.argv[1:]:
        try:
            problems = check_file(p)
        except SyntaxError as e:
            print("SYNTAX", e)
            bad += 1
            continue
        for pr in problems:
            print(pr)
        bad += len(problems)
        print(f"{p}: {'OK' if not problems else str(len(problems)) + ' problem(s)'}")
    sys.exit(1 if bad else 0)
