import { IllegalCharError, MissingSyntaxError, UnclosedDelimiterError } from "./errors"
import { Context, Token } from "./defs"

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
        
        // Multi line comments
        } else if (ctx.src.startsWith("-[", ctx.c)) {
            lexMultiLineComment(ctx)

        } else if (ctx.src.startsWith("-", ctx.c)) {
            addToken(ctx, "-", ctx.c)
        
        // Types
        } else if (ctx.src.startsWith(":", ctx.c)) {
            addToken(ctx, ":", ctx.c)

        } else if (ctx.src.startsWith("|", ctx.c)) {
            addToken(ctx, "|", ctx.c)

        } else if (ctx.src.startsWith("&", ctx.c)) {
            addToken(ctx, "&", ctx.c)

            

        // } else if (ctx.src.startsWith("Int64", ctx.c)) {
        //     addToken(ctx, "INT64", ctx.c + 4)
        
        // } else if (ctx.src.startsWith("Dec64", ctx.c)) {
        //     addToken(ctx, "DEC64", ctx.c + 4)

        // } else if (ctx.src.startsWith("Bool", ctx.c)) {
        //     addToken(ctx, "BOOL", ctx.c + 3)
            
        // Operators
        } else if (ctx.src.startsWith(",", ctx.c)) {
            addToken(ctx, ",", ctx.c)

        } else if (ctx.src.startsWith(".", ctx.c)) {
            addToken(ctx, ".", ctx.c)

        } else if (ctx.src.startsWith("==", ctx.c)) {
            addToken(ctx, "==", ctx.c + 1)

        } else if (ctx.src.startsWith("=", ctx.c)) {
            addToken(ctx, "=", ctx.c)
        
        } else if (ctx.src.startsWith("+", ctx.c)) {
            addToken(ctx, "+", ctx.c)

        } else if (ctx.src.startsWith("-", ctx.c)) {
            addToken(ctx, "-", ctx.c)

        } else if (ctx.src.startsWith("*", ctx.c)) {
            addToken(ctx, "*", ctx.c)

        } else if (ctx.src.startsWith("/", ctx.c)) {
            addToken(ctx, "/", ctx.c)

        } else if (c === '(') {
            addToken(ctx, "(", ctx.c)

        } else if (c === ')') {
            addToken(ctx, ")", ctx.c)

        // Keywords
        } else if (ctx.src.startsWith("let", ctx.c)) {
            addToken(ctx, "let", ctx.c + 2)
            if (c !== ' ' && c !== '\n' && c !== '\t' && c !== '\r') {
                let err = new MissingSyntaxError("space after keyword", ctx.c + 2, ctx.line)
            }

        // Identifier or keyword
        } else if (isAlphabet(c) || c === "_") {
            let anchor = ctx.c
            let id = lexIdentifier(ctx)
            
            if (ctx.errors.length === 0) {
                let tok = new Token('IDENTIFIER', anchor, ctx.c, ctx.line)
                tok.value = id
                ctx.tokens.push(tok)
            }

            
            
        // Illegal characters
        } else {
            let illegalTokenErr = new IllegalCharError(c, ctx.c, ctx.line)
            ctx.errors.push(illegalTokenErr)
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
            let err = new MissingSyntaxError("identifier after '.'", ctx.c, ctx.line)
            ctx.errors.push(err)
        }
        qualifiedName += "." + next
    }
    return qualifiedName
}

const lexIdsandKeywords = (ctx: Context) => {
    let cursor = ctx.c
    let c = ctx.src[cursor]
    while (cursor < ctx.src.length && (
        isAlphabet(c) 
        || isDigit(c) 
        || c === "_")
    ) { 
        cursor++
        c = ctx.src[cursor]
    }
    let buffer = ctx.src.substring(ctx.c, cursor)
    switch (buffer) {
        case 'let':
            addToken(ctx, 'let', cursor - 1)
            break

        default:
            if (ctx.errors.length === 0) {
                let tok = new Token('IDENTIFIER', ctx.c, cursor - 1, ctx.line)
                tok.value = buffer
                ctx.tokens.push(tok)
            }
            break
    }
    ctx.c = cursor
}

const lexNumbers = (ctx: Context) => {
    let cursor = ctx.c
    let c = ctx.src[cursor]
    let isFloat = false

    while (cursor < ctx.src.length && (
        isDigit(c) || c === "." || c === "_"
    )) {
        if (c === "." && isFloat) {
            let error = new IllegalCharError(c, cursor, ctx.line)
            ctx.errors.push(error)
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
        let unclosedCommentErr = new UnclosedDelimiterError("multi-line comment", "-[", anchorPos, anchorLine)
        ctx.errors.push(unclosedCommentErr)
    }
}