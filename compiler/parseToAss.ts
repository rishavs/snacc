import { 
    BinaryLeaf, DeclarationLeaf, FloatLeaf, FunCallLeaf, GroupedExprLeaf, 
    IdentifierLeaf, IntLeaf, Leaf, PrimaryTypeLeaf, RootLeaf, TypeChainExprLeaf, 
    UnaryLeaf 
} from "./ass";
import { Context, Token } from "./defs";

function pushToAss<T extends Leaf, A extends any[]>(ctx: Context, cls: new (...args: A) => T, ...args: A): T {
    let leaf = new cls(...args);
    leaf.self = ctx.ass.push(leaf);

    return leaf;
}

export const parseFileToAss = (ctx: Context) => {
    ctx.t = 0;
    ctx.line = 0;

    let root = new RootLeaf(0,0, ctx.filepath)
    root.self = ctx.ass.push(root) - 1

    let stmt : Leaf | Error
    let token : Token
    let nextToken : Token

    while (ctx.t < ctx.tokens.length) {
        token = ctx.tokens[ctx.t]
        nextToken = ctx.tokens[ctx.t + 1]
        
        if (token.name === "let") {
            stmt = parseDeclarationStmt(ctx)
        } else if (token.name === "IDENTIFIER" && nextToken && nextToken.name === "=") {
            stmt = parseDeclarationStmt(ctx)
        } else if (token.name === "IDENTIFIER" && nextToken && nextToken.name === "(") {
            stmt = parseFuncCall(ctx)
        } else {
            console.log("Parsing Expression")
            stmt = parseExpression(ctx)
            // stmt = new MissingSyntaxError("Statement", token.start, token.line)
        }

        if (stmt instanceof Leaf) {
            stmt.parent = root.self;
            root.statements.push(stmt.self)

        } else {
            ctx.errors.push(stmt);
            recover(ctx);
        }
    }
}

export const parseDeclarationStmt = (ctx: Context): DeclarationLeaf | Error => {
    let decl = new DeclarationLeaf(ctx.tokens[ctx.t].start, ctx.tokens[ctx.t].line);
    decl.self = ctx.ass.push(decl) - 1

    if (ctx.tokens[ctx.t].name === "let") {
        ctx.t++; // consume 'let'
        decl.isNewDeclaration = true;
    }
        
    if (ctx.tokens[ctx.t].name !== "IDENTIFIER") {
        let err = new Error("Expected an Identifier for the declaration statement at Line:" + ctx.line + " & Pos:" + ctx.tokens[ctx.t].start)
        err.name = "SyntaxError"
        return err;
    }

    let identifier = parseIdentifier(ctx);
    if (identifier instanceof Error) return identifier;
    identifier.parent = decl.self
    decl.id = identifier.self;

    if (ctx.tokens[ctx.t].name !== ":") {
        console.log(ctx.tokens[ctx.t])
        let err = new Error("Expected a ':' after the identifier for the Type declaration at Line:" + ctx.line + " & Pos:" + ctx.tokens[ctx.t].start)
        err.name = "SyntaxError"
        return err;
    }
    ctx.t++; // consume ':'
    let typeExpr = parseTypeExpression(ctx);
    if (typeExpr instanceof Error) return typeExpr;
    typeExpr.parent = decl.self
    decl.declaredType = typeExpr.self

    if (ctx.tokens[ctx.t].name === "=") {
        // Handle assignment
        ctx.t++; // consume '='
        let expr = parseExpression(ctx);
        if (expr instanceof Error) return expr;
        decl.expression = expr.self
    } 
    // Only declaration
    return decl
}

// eg. `let x : Int | Dec = 10.2`
// A type expression is a chain of types separated by either an 'AND' or 'OR' operator
// A chain can have only operator kind
// A type expression will not handle anonymous/object types. they should be declared separately
export const parseTypeExpression = (ctx: Context, opr?: 'OR' |'AND'): TypeChainExprLeaf | Error => {
    let typeExpr = new TypeChainExprLeaf(ctx.tokens[ctx.t].start, ctx.tokens[ctx.t].line);
    typeExpr.self = ctx.ass.push(typeExpr) - 1

    // Leading opr is allowed
    if (ctx.tokens[ctx.t].name === "|" ) {
        typeExpr.operator = 'OR';
        ctx.t++; // consume operator
    } else if (ctx.tokens[ctx.t].name === "&" ) {
        typeExpr.operator = 'AND';
        ctx.t++; // consume operator
    }  
    if (!ctx.tokens[ctx.t] || (ctx.tokens[ctx.t].name !== "IDENTIFIER")) {
        let err = new Error("Expected a Type declaration at Line:" + ctx.line + " & Pos:" + ctx.tokens[ctx.t].start)
        err.name = "SyntaxError"
        return err;
    }

    let first = new PrimaryTypeLeaf(ctx.tokens[ctx.t].start, ctx.tokens[ctx.t].line, ctx.tokens[ctx.t].value!);
    first.self = ctx.ass.push(first) - 1
    first.parent = typeExpr.self
    typeExpr.types.push (first.self);
    ctx.t++; // consume type
    
    while (ctx.t < ctx.tokens.length && (
        ctx.tokens[ctx.t].name === "|" || 
        ctx.tokens[ctx.t].name === "&" || 
        ctx.tokens[ctx.t].name === "IDENTIFIER")
    ) {
        if (ctx.tokens[ctx.t].name === "|" ) {
            if (typeExpr.operator === 'AND') {
                let err = new Error("Cannot mix AND and OR operators in a type declaration");
                err.name = "SyntaxError"
                return err;
            } 
            typeExpr.operator = 'OR';
            ctx.t++; // consume operator 
        } else if (ctx.tokens[ctx.t].name === "&" ) {
            if (typeExpr.operator === 'OR') {
                let err = new Error("Cannot mix AND and OR operators in a type declaration");
                err.name = "SyntaxError"
                return err;
            } 
            typeExpr.operator = 'AND';
            ctx.t++; // consume operator
        }

        if (ctx.tokens[ctx.t].name !== "IDENTIFIER") {
            let err = new Error("Expected a Type declaration after the type operator at Line:" + ctx.line + " & Pos:" + ctx.tokens[ctx.t].start)
            err.name = "SyntaxError"
            return err;
        }

        let next = new PrimaryTypeLeaf(ctx.tokens[ctx.t].start, ctx.tokens[ctx.t].line, ctx.tokens[ctx.t].value!);
        next.self = ctx.ass.push(next) - 1
        next.parent = typeExpr.self
        typeExpr.types.push (next.self);
        ctx.t++; // consume type
    }
    return typeExpr;
}

export const parseExpression = (ctx: Context): Leaf | Error => {
    return parseBinaryExpr(ctx);
}

export const parseUnaryExpr = (ctx: Context): Leaf | Error => {
    let token = ctx.tokens[ctx.t];
    let unary =  new UnaryLeaf(token.start, token.line, token.name);
    unary.self = ctx.ass.push(unary) - 1

    if (token.name === "-" || token.name === "!") {
        ctx.t++; // consume '-'
        let expr = parsePrimaryExpr(ctx);
        if (expr instanceof Error) return expr;
        expr.self = ctx.ass.push(expr) - 1
        unary.right = expr.self
        expr.parent = unary.self
        return unary;
    } else {
        return parsePrimaryExpr(ctx);
    }
}

const getPrecedence = (token: Token): number => {
    if (!token) return 0;
    
    switch (token.name) {
        case "*":
        case "/":
            return 2;
        case "+":
        case "-":
            return 1;
        default:
            return 0;
    }
}

export const parseBinaryExpr = (ctx: Context, precedence = 0): Leaf | Error => {
    let left = parseUnaryExpr(ctx);
    if (left instanceof Error) return left;

    while (true) {
        let token = ctx.tokens[ctx.t];
        let tokenPrecedence = getPrecedence(token);
        if (tokenPrecedence <= precedence) break;

        ctx.t++; // consume operator
        let right = parseBinaryExpr(ctx, tokenPrecedence);
        if (right instanceof Error) return right;
        if (left instanceof Error) return left;

        let node: BinaryLeaf = new BinaryLeaf(left.start, left.line, token.name);
        node.self = ctx.ass.push(node) - 1
        left.parent = node.self
        node.left = left.self;
        right.parent = node.self
        node.right = right.self;
        left = node;
    }

    return left;
}



export const parsePrimaryExpr = (ctx: Context): Leaf | Error => {
    let token = ctx.tokens[ctx.t];
    let nextToken = ctx.tokens[ctx.t + 1];

    if   (token.name === "IDENTIFIER" && nextToken && nextToken.name === "(") {
        return parseFuncCall(ctx);

    } else if (token.name === "IDENTIFIER") {
        return parseIdentifier(ctx);
    
    } else if   (token.name === "INT" || token.name === "FLOAT") {
        return parseNumber(ctx);

    } else if   (token.name === "let") {
        return parseDeclarationStmt(ctx);

    } else if (token.name === "(") {
        return parseGroupedExpr(ctx);

    } else {
        let err = new Error("Expected an expression at Line:" + ctx.line + " & Pos:" + token.start)
        err.name = "SyntaxError"
        return err;
    }
}

export const parseGroupedExpr = (ctx: Context): GroupedExprLeaf | Error => {
    let token = ctx.tokens[ctx.t];
    if (token.name !== "(") {
        let err = new Error("Expected an opening parenthesis '(' for the grouped expression at Line:" + ctx.line + " & Pos:" + token.start)
        err.name = "SyntaxError"
        return err;
    }
    let group = new GroupedExprLeaf(token.start, token.line);
    group.self = ctx.ass.push(group) - 1
    ctx.t++; // consume '('

    // Parse the enclosed expression
    let expr = parseExpression(ctx);
    if (expr instanceof Error) return expr;
    expr.parent = group.self
    group.expression = expr.self

    // Check for the closing parenthesis ')'
    token = ctx.tokens[ctx.t];
    if (token.name !== ")") {
        let err = new Error("Expected a closing parenthesis ')' for the grouped expression at Line:" + ctx.line + " & Pos:" + token.start)
        err.name = "SyntaxError"
        return err;
    }
    ctx.t++; // consume ')'

    return group
}

export const parseFuncCall = (ctx: Context): FunCallLeaf | Error => {
    let token = ctx.tokens[ctx.t];
    let funCallLeaf = new FunCallLeaf(token.start, token.line);
    funCallLeaf.self = ctx.ass.push(funCallLeaf) - 1

    if (token.name !== "IDENTIFIER") {
        let err = new Error("Expected an Identifier for the function call at Line:" + ctx.line + " & Pos:" + token.start)
        err.name = "SyntaxError"
        return err;
    }
    let id = parseIdentifier(ctx);
    if (id instanceof Error) return id;
    funCallLeaf.id = id.self
    id.parent = funCallLeaf.self

    if (ctx.tokens[ctx.t].name !== "(") {
        let err = new Error("Expected an opening parenthesis '(' for the function call at Line:" + ctx.line + " & Pos:" + token.start)
        err.name = "SyntaxError"
        return err;
    }
    ctx.t++; // consume '('

    funCallLeaf.expressions = [];
    if (ctx.tokens[ctx.t].name === ")") {
        ctx.t++; // consume ')'
        return funCallLeaf; // empty args list
    }

    // Leading comma is allowed
    if (ctx.tokens[ctx.t].name === ",") {
        ctx.t++; // consume ','
    }

    while (ctx.t < ctx.tokens.length && ctx.tokens[ctx.t].name !== "EOS") {
        let expr = parseExpression(ctx);
        if (expr instanceof Error) return expr;
        expr.parent = funCallLeaf.self;
        funCallLeaf.expressions.push(expr.self);

        if (ctx.tokens[ctx.t].name === ")") break;
        if (ctx.tokens[ctx.t].name !== ",") {
            let err = new Error("Expected a comma ',' for the arguments of function call at Line:" + ctx.line + " & Pos:" + token.start)
            err.name = "SyntaxError"
            return err
        }
        ctx.t++; // consume ','
    }
    if (ctx.tokens[ctx.t].name !== ")") {
        let err = new Error("Expected a closing parenthesis ')' for the function call at Line:" + ctx.line + " & Pos:" + token.start)
        err.name = "SyntaxError"
        return err;
    }
    ctx.t++; // consume ')'

    return funCallLeaf
}

export const parseIdentifier = (ctx: Context): IdentifierLeaf | Error => {
    let token = ctx.tokens[ctx.t];
    if (token.name !== "IDENTIFIER") {
        let err = new Error("Expected an Identifier at Line:" + ctx.line + " & Pos:" + token.start)
        err.name = "SyntaxError"
        return err;
    }
    let leaf = new IdentifierLeaf(token.start, token.end, token.value!);
    leaf.self = ctx.ass.push(leaf) - 1

    ctx.t++; // consume identifier
    return leaf;
}

export const parseNumber = (ctx: Context): IntLeaf | FloatLeaf | Error => {
    let token = ctx.tokens[ctx.t];
    if (token.name !== 'INT' && token.name !== 'FLOAT') {
        let err = new Error("Expected a Number at Line:" + ctx.line + " & Pos:" + token.start)
        err.name = "SyntaxError"
        return err;
    }

    let leaf : IntLeaf | FloatLeaf;
    if (token.name === 'INT') {
        leaf = new IntLeaf(token.start, token.end, token.value!);
    } else {
        leaf = new FloatLeaf(token.start, token.end, token.value!);
    }
    leaf.self = ctx.ass.push(leaf) - 1

    ctx.t++; // consume number
    return leaf;
}


const recover = (ctx: Context) => { 
    while (
        ctx.t < ctx.tokens.length && 
        ctx.tokens[ctx.t].name !== "EOS" && 
        !['let', 'if'].includes(ctx.tokens[ctx.t].name)
    ) {
        ctx.t++;
    }
}