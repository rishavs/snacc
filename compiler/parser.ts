import { 
    DeclarationNode, IdentifierNode, FunCallNode, Node, BinaryNode,
    UnaryNode, IntNode, FloatNode,
    GroupedExprNode,
    PrimaryTypeNode,
    TypeChainExprNode
} from "./ast";
import { builtinTypes, Context, Token } from "./defs";

export const parseFile = (ctx: Context) => {
    ctx.t = 0;
    ctx.line = 0;

    let stmt : Node | Error
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

        if (stmt instanceof Node) {
            stmt.parent = ctx.root;
            ctx.root.statements.push(stmt);
        } else {
            ctx.errors.push(stmt);
            recover(ctx);
        }
    }
}

export const parseDeclarationStmt = (ctx: Context): DeclarationNode | Error => {
    let decl = new DeclarationNode(ctx.tokens[ctx.t].start, ctx.tokens[ctx.t].line);

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
    decl.id = identifier;

    // parse type expression
    if (ctx.tokens[ctx.t].name === ":") {
        ctx.t++; // consume ':'
        let typeExpr = parseTypeExpression(ctx);
        if (typeExpr instanceof Error) return typeExpr;
        decl.declaredType = typeExpr;
    }
    
    // parse assignment
    if (ctx.tokens[ctx.t].name === "=") {
        // Handle assignment
        ctx.t++; // consume '='
        let expr = parseExpression(ctx);
        if (expr instanceof Error) return expr;
        decl.expression = expr;
    } 

    // Only declaration
    return decl
}

// eg. `let x : Int | Dec = 10.2`
// A type expression is a chain of types separated by either an 'AND' or 'OR' operator
// A chain can have only operator kind
// A type expression will not handle anonymous/object types. they should be declared separately
export const parseTypeExpression = (ctx: Context, opr?: 'OR' |'AND'): TypeChainExprNode | Error => {
    let typeExpr = new TypeChainExprNode(ctx.tokens[ctx.t].start, ctx.tokens[ctx.t].line);

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

    let first = new PrimaryTypeNode(ctx.tokens[ctx.t].start, ctx.tokens[ctx.t].line, ctx.tokens[ctx.t].value!);
    typeExpr.types.push (first.value);
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

        let next = new PrimaryTypeNode(ctx.tokens[ctx.t].start, ctx.tokens[ctx.t].line, ctx.tokens[ctx.t].value!);
        typeExpr.types.push (next.value);
        ctx.t++; // consume type
    }
    return typeExpr;
}

export const parseExpression = (ctx: Context): Node | Error => {
    return parseBinaryExpr(ctx);
}

export const parseUnaryExpr = (ctx: Context): Node | Error => {
    let token = ctx.tokens[ctx.t];

    if (token.name === "-" || token.name === "!") {
        ctx.t++; // consume '-'
        let expr = parsePrimaryExpr(ctx);
        if (expr instanceof Error) return expr;
        let unary =  new UnaryNode(token.start, token.line, token.name, expr);
        expr.parent = unary;
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

export const parseBinaryExpr = (ctx: Context, precedence = 0): Node | Error => {
    let left = parseUnaryExpr(ctx);
    if (left instanceof Error) return left;

    while (true) {
        let token = ctx.tokens[ctx.t];
        let tokenPrecedence = getPrecedence(token);
        if (tokenPrecedence <= precedence) break;

        ctx.t++; // consume operator
        let right = parseBinaryExpr(ctx, tokenPrecedence);
        if (right instanceof Error) return right;

        let node: BinaryNode = new BinaryNode(left.start, left.line, token.name, left, right);
        node.end = right.end;
        left = node;
    }

    return left;
}


export const parsePrimaryExpr = (ctx: Context): Node | Error => {
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

export const parseGroupedExpr = (ctx: Context): GroupedExprNode | Error => {
    let token = ctx.tokens[ctx.t];
    if (token.name !== "(") {
        let err = new Error("Expected an opening parenthesis '(' for the grouped expression at Line:" + ctx.line + " & Pos:" + token.start)
        err.name = "SyntaxError"
        return err;
    }
    ctx.t++; // consume '('

    // Parse the enclosed expression
    let expr = parseExpression(ctx);
    if (expr instanceof Error) return expr;

    // Check for the closing parenthesis ')'
    token = ctx.tokens[ctx.t];
    if (token.name !== ")") {
        let err = new Error("Expected a closing parenthesis ')' for the grouped expression at Line:" + ctx.line + " & Pos:" + token.start)
        err.name = "SyntaxError"
        return err;
    }
    ctx.t++; // consume ')'

    return new GroupedExprNode(token.start, token.line, expr);
}

export const parseFuncCall = (ctx: Context): FunCallNode | Error => {
    let token = ctx.tokens[ctx.t];
    let funCallNode = new FunCallNode(token.start, token.line);

    if (token.name !== "IDENTIFIER") {
        let err = new Error("Expected an Identifier for the function call at Line:" + ctx.line + " & Pos:" + token.start)
        err.name = "SyntaxError"
        return err;
    }
    let id = parseIdentifier(ctx);
    if (id instanceof Error) return id;
    funCallNode.id = id;

    if (ctx.tokens[ctx.t].name !== "(") {
        let err = new Error("Expected an opening parenthesis '(' for the function call at Line:" + ctx.line + " & Pos:" + token.start)
        err.name = "SyntaxError"
        return err;
    }
    ctx.t++; // consume '('

    funCallNode.expressions = [];
    if (ctx.tokens[ctx.t].name === ")") {
        ctx.t++; // consume ')'
        return funCallNode; // empty args list
    }

    // Leading comma is allowed
    if (ctx.tokens[ctx.t].name === ",") {
        ctx.t++; // consume ','
    }

    while (ctx.t < ctx.tokens.length && ctx.tokens[ctx.t].name !== "EOS") {
        let expr = parseExpression(ctx);
        if (expr instanceof Error) return expr;
        expr.parent = funCallNode;
        funCallNode.expressions.push(expr);

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

    return funCallNode
}

export const parseIdentifier = (ctx: Context): IdentifierNode | Error => {
    let token = ctx.tokens[ctx.t];
    if (token.name !== "IDENTIFIER") {
        let err = new Error("Expected an Identifier at Line:" + ctx.line + " & Pos:" + token.start)
        err.name = "SyntaxError"
        return err;
    }
    let node = new IdentifierNode(token.start, token.end, token.value!);
    ctx.t++; // consume identifier
    return node;
}

export const parseNumber = (ctx: Context): IntNode | FloatNode | Error => {
    let token = ctx.tokens[ctx.t];
    let node : IntNode | FloatNode;
    if (token.name === 'INT') {
        node = new IntNode(token.start, token.end, token.value!);
        ctx.t++; // consume number
    } else if (token.name === 'FLOAT') {
        node = new FloatNode(token.start, token.end, token.value!);
        ctx.t++; // consume number
    } else {
        let err = new Error("Expected a Number at Line:" + ctx.line + " & Pos:" + token.start)
        err.name = "SyntaxError"
        return err;
    }
    return node;
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