import { Context, keywords, operators, Token } from "./defs"

const isAlphabet = (c: string) => {
    return (c >= "a" && c <= "z") || (c >= "A" && c <= "Z")
}

const isDigit = (c: string) => {
    return c >= "0" && c <= "9"
}

const addToken = (ctx: Context, name: string, end: number) => {
    if (ctx.errors.length === 0) ctx.tokens.push(new Token(name, ctx.c, end, ctx.line))
    ctx.c = end + 1
}

const skipWhitespace = (ctx: Context) => {
    while (ctx.c < ctx.src.length && (
        ctx.src[ctx.c] === ' ' || ctx.src[ctx.c] === '\n' || ctx.src[ctx.c] === '\t' || ctx.src[ctx.c] === '\r')
    ) {
        if (ctx.src[ctx.c] === '\n') {
            ctx.line++
        }
        ctx.c++
    }
}

export const lexFile = (ctx: Context) => {
    let start = Date.now()

    let c = ctx.src[ctx.c]
    while (ctx.c < ctx.src.length) {
        c = ctx.src[ctx.c]

        // Handle whitespace
        if (c === ' ' || c === '\n' || c === '\t' || c === '\r') {
            skipWhitespace(ctx)

        // Numbers
        } else if (isDigit(c)) {
            lexNumbers(ctx)

        // Single line comments
        } else if (ctx.src.startsWith("--", ctx.c)) {
            while (ctx.c < ctx.src.length && ctx.src[ctx.c] !== '\n') {
                ctx.c++
            }
        
        // Multi line Comments
        } else if (ctx.src.startsWith("-[", ctx.c)) {
            lexMultiLineComment(ctx)

         // Identifier or keyword
        } else if (isAlphabet(c) || c === "_") {
            let found = false
            for (let kwd of keywords) {
                if (ctx.src.startsWith(kwd, ctx.c)) {
                    addToken(ctx, kwd, ctx.c + kwd.length - 1)
                    found = true
                    c = ctx.src[ctx.c]
                    if (c !== ' ' && c !== '\n' && c !== '\t' && c !== '\r') {
                        let err = new Error(`Expected a space after the keyword "${kwd}" at Line:${ctx.line} & Pos:${ctx.c}`)
                        err.name = "SyntaxError"
                        ctx.errors.push(err)
                    }
                    break
                }
            } 
            if (found) continue

            let anchor = ctx.c
            let id = lexIdentifier(ctx)
            
            if (ctx.errors.length === 0) {
                let tok = new Token('IDENTIFIER', anchor, ctx.c, ctx.line)
                tok.value = id
                ctx.tokens.push(tok)
            }
            
        // Illegal characters
        } else {
            let found = false
            for (let opr of operators) {
                if (ctx.src.startsWith(opr, ctx.c)) {
                    addToken(ctx, opr, ctx.c + opr.length - 1)
                    found = true
                    break
                }
            }
            if (found) continue

            // Add error for illegal characters
            let err = new Error(`Found an unexpected character "${c}" at Line:${ctx.line} & Pos:${ctx.c}`)
            err.name = "SyntaxError"
            ctx.errors.push(err)
            ctx.c++
        }
    }
    
    // if (ctx.errors.length === 0) ctx.tokens.push(new Token("EOS", ctx.c, ctx.c, ctx.line))
    ctx.lexingDuration = Date.now() - start
}

const lexIdentifier = (ctx: Context): string => {
    let cursor = ctx.c
    let c = ctx.src[cursor]
    let qualifiedName = ''

    while (cursor < ctx.src.length && (
        isAlphabet(c) 
        || isDigit(c) 
        || c === "_")
    ) { 
        cursor++
        c = ctx.src[cursor]
    }
    qualifiedName += ctx.src.substring(ctx.c, cursor)
    ctx.c = cursor

    // optional whitespace
    skipWhitespace(ctx)

    // Check if qualified name
    if (ctx.src.startsWith(".", ctx.c)) {
        ctx.c++
        skipWhitespace(ctx)
        let next = lexIdentifier(ctx)
        if (next === '') {
            let err = new Error(`Expected a Qualified Identifier fragment after the "." at Line:${ctx.line} & Pos:${cursor}`)
            err.name = "SyntaxError"
            ctx.errors.push(err)
        }
        qualifiedName += "." + next
    }
    return qualifiedName
}

const lexNumbers = (ctx: Context) => {
    let cursor = ctx.c
    let c = ctx.src[cursor]
    let isFloat = false

    while (cursor < ctx.src.length && (
        isDigit(c) || c === "." || c === "_"
    )) {
        if (c === "." && isFloat) {
            let err = new Error(`Found an unexpected "." at Line:${ctx.line} & Pos:${cursor}`)
            err.name = "SyntaxError"
            ctx.errors.push(err)
            cursor++
            break
        }

        if (c === "_") {
            cursor++
        } else if (c === "." && !isFloat) {
            isFloat = true
            cursor++
        } else {
            cursor++  // Increment cursor for digits and other characters
        }
        c = ctx.src[cursor]
    }
    let tok = new Token(isFloat ? "FLOAT" : "INT", ctx.c, cursor - 1, ctx.line)
    tok.value = ctx.src.substring(ctx.c, cursor).replace(/_/g, "")
    tok.end = cursor - 1
    ctx.tokens.push(tok)
    ctx.c = cursor
}

const lexMultiLineComment = (ctx: Context) => {
    let delimClosed = false
    let anchorPos = ctx.c
    let anchorLine = ctx.line
    while (ctx.c < ctx.src.length) {
        if (ctx.src[ctx.c] === '\n') {
            ctx.line++
        }
        if (ctx.src.startsWith("]-", ctx.c)) {
            ctx.c += 2
            delimClosed = true
            break
        } 
        ctx.c++
    }
    if (!delimClosed) {
        let err = new Error(`The multi-line comment starting at Line:${anchorLine} & Pos:${anchorPos}, is not closed`)
        err.name = "SyntaxError"
        ctx.errors.push(err)
    }
}