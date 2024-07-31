import { 
    DeclarationNode, ExpressionNode, IdentifierNode, NumberNode, StatementNode, 
    BinaryExpressionNode, UnaryExpressionNode, 
    FunCallNode, BlockNode, Node
} from "./ast";
import { Context, Token } from "./defs";
import { UnknownError, MissingSpecificTokenError, MissingSyntaxError, UnclosedDelimiterError } from "./errors";

export const parseFile = (ctx: Context) => {
    let block = parseBlock(ctx, ctx.root);
    if (block instanceof BlockNode) {
        ctx.root.block = block;
    } else if (block instanceof Array) {
        ctx.errors.push(...block);
    } else {
        ctx.errors.push(new UnknownError(ctx.tokens[ctx.t]?.line, ctx.tokens[ctx.t]?.start));
    }
}

export const parseBlock = (ctx: Context, parent: Node): BlockNode | Error[] => {
    let errors: Error[] = [];
    let token: Token = ctx.tokens[ctx.t];
    let block = new BlockNode(token.start, token.line);
    block.statements = []
    block.parent = parent;
    while (ctx.t < ctx.tokens.length && ctx.tokens[ctx.t].name !== "EOS") {
        let statement = parseStatement(ctx, block);
        if (statement instanceof Node) {
            statement.parent = block;
            block.statements.push(statement);
        } else if (statement instanceof Error) {
            errors.push(statement);
            recover(ctx);
        } else {
            errors.push(new UnknownError(token.line, token.start));
            recover(ctx);
        }
    }
    if (errors.length === 0) return block;
    else return errors;
}

export const parseStatement = (ctx: Context, parent: BlockNode): StatementNode | Error => {
    const token = ctx.tokens[ctx.t];
    switch (token.name) {
        case "let":
            return parseDeclarationStmt(ctx);
        case "IDENTIFIER":
            return parseIdentifierOrFuncCall(ctx);
        default:
            return new MissingSyntaxError("Statement", token.start, token.line);
    }
}

const parseIdentifierOrFuncCall = (ctx: Context): StatementNode | Error => {
    const nextToken = ctx.tokens[ctx.t + 1];
    if (!nextToken) {
        return new MissingSyntaxError("Statement", ctx.tokens[ctx.t].start, ctx.tokens[ctx.t].line);
    }
    switch (nextToken.name) {
        case "=":
            return parseAssignmentStmt(ctx);
        case "(":
            return parseFuncCall(ctx);
        default:
            return new MissingSyntaxError("Statement", ctx.tokens[ctx.t].start, ctx.tokens[ctx.t].line);
    }
}

export const parseDeclarationStmt = (ctx: Context): DeclarationNode | Error => {
    let token = ctx.tokens[ctx.t];
    if (ctx.tokens[ctx.t].name !== "let") {
        return new MissingSpecificTokenError('declaration statement', 'let', ctx.tokens[ctx.t].start, ctx.tokens[ctx.t].line);
    }
    ctx.t++; // consume 'let'
    if (ctx.tokens[ctx.t].name !== "IDENTIFIER") {
        return new MissingSpecificTokenError('declaration statement', 'identifier', ctx.tokens[ctx.t].start, ctx.tokens[ctx.t].line);
    }

    let decl = new DeclarationNode(token.start, token.line);
    let identifier = parseIdentifier(ctx);
    if (identifier instanceof Error) return identifier;
    
    decl.identifier = identifier;
    if (ctx.tokens[ctx.t].name === "=") {
        // Handle assignment
        ctx.t++; // consume '='
        let assignment = parseExpression(ctx);
        if (assignment instanceof Error) return assignment;
        decl.assignment = assignment;
    } 
    // Only declaration
    return decl
}

const parseAssignmentStmt = (ctx: Context): DeclarationNode | Error => {
    let token = ctx.tokens[ctx.t];
    let assignNode = new DeclarationNode(token.start, token.line);
    
    let identifier = parseIdentifier(ctx);
    if (identifier instanceof Error) return identifier;
    assignNode.identifier = identifier;
    
    if (ctx.tokens[ctx.t].name !== "=") {
      return new MissingSpecificTokenError('assignment', '=', ctx.tokens[ctx.t].start, ctx.tokens[ctx.t].line);
    }
    ctx.t++; // consume '='
    
    let expr = parseExpression(ctx);
    if (expr instanceof Error) return expr;
    assignNode.assignment = expr;
    
    return assignNode;
}

export const parseExpression = (ctx: Context): ExpressionNode | Error => {
    return parseBinaryExpr(ctx);
}

export const parseBinaryExpr = (ctx: Context, minPrecedence: number = 0): ExpressionNode | Error => {
    let left = parseUnaryExpr(ctx);

    while (true) {
        let token = ctx.tokens[ctx.t];
        let precedence = getBinaryOpPrecedence(token.name);
        if (precedence < minPrecedence) break;

        ctx.t++; // consume operator
        let right = parseBinaryExpr(ctx, precedence + 1);
        if (right instanceof Error) return right;

        if (left instanceof Error) return left;
        let node = new BinaryExpressionNode(left.start, left.line);
        node.operator = token.name;
        node.left = left;
        node.right = right;
        node.end = right.end;
        left = node;
    }

    return left;
}

export const parseUnaryExpr = (ctx: Context): UnaryExpressionNode | Error => {
    let token = ctx.tokens[ctx.t];
    let isUnary = false

    if (token.name === "-" || token.name === "!") {
        ctx.t++; // consume '-' or '!'
        isUnary = true;
    }
    let expr = parsePrimaryExpr(ctx);
    if (expr instanceof Error) return expr;

    if (!isUnary) return expr;

    let unary = new UnaryExpressionNode(token.start, token.line);
    unary.operator = token.name;
    unary.right = expr;
    expr.parent = unary;
    return unary;
}

export const parsePrimaryExpr = (ctx: Context): ExpressionNode | Error => {
    let token = ctx.tokens[ctx.t];
    switch (token.name) {
        case "IDENTIFIER":
            return parseIdentifierOrFuncCall(ctx);
        case "NUMBER":
            return parseNumber(ctx);
        case "(":
            ctx.t++; // consume '('
            let expr = parseExpression(ctx);
            if (expr instanceof Error) return expr;
            if (ctx.tokens[ctx.t].name !== ")") {
                return new MissingSpecificTokenError('grouped expression', ')', ctx.tokens[ctx.t].start, ctx.tokens[ctx.t].line);
            }
            ctx.t++; // consume ')'
            return expr;
        default:
            return new MissingSyntaxError("expression", token.start, token.line);
    }
}

export const parseFuncCall = (ctx: Context): FunCallNode | Error => {
    let token = ctx.tokens[ctx.t];
    let funCallNode = new FunCallNode(token.start, token.line);

    if (token.name !== "IDENTIFIER") {
        return new MissingSpecificTokenError('function call', 'identifier', token.start, token.line);
    }
    let id = parseIdentifier(ctx);
    if (id instanceof Error) return id;
    funCallNode.id = id;

    if (ctx.tokens[ctx.t].name !== "(") {
        return new MissingSpecificTokenError('list of arguments', '(', ctx.tokens[ctx.t].start, ctx.tokens[ctx.t].line);
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
            return new MissingSpecificTokenError('list of arguments', ',', ctx.tokens[ctx.t].start, ctx.tokens[ctx.t].line);
        }
        ctx.t++; // consume ','
    }
    if (ctx.tokens[ctx.t].name !== ")") {
        return new UnclosedDelimiterError('list of arguments', ')', ctx.tokens[ctx.t].start, ctx.tokens[ctx.t].line);
    }
    ctx.t++; // consume ')'

    return funCallNode
}

export const parseIdentifier = (ctx: Context): IdentifierNode | Error => {
    let token = ctx.tokens[ctx.t];
    if (token.name !== "IDENTIFIER") {
        return new MissingSyntaxError("Identifier", token.start, token.line);
    }
    let node = new IdentifierNode(token.start, token.end);
    node.value = token.value!;
    ctx.t++; // consume identifier

    while (ctx.t < ctx.tokens.length && ctx.tokens[ctx.t].name === ".") {
        ctx.t++; // consume '.'
        if (ctx.tokens[ctx.t].name !== "IDENTIFIER") {
            return new MissingSpecificTokenError("Qualified Identifier", "identifier", ctx.tokens[ctx.t].start, ctx.tokens[ctx.t].line);
        }
        node.isQualified = true;
        node.value += `.${ctx.tokens[ctx.t].value}`;
        ctx.t++; // consume identifier
    }

    return node;
}

export const parseNumber = (ctx: Context): NumberNode | Error => {
    let token = ctx.tokens[ctx.t];
    if (token.name !== "NUMBER") {
        return new MissingSyntaxError("Number", token.start, token.line);
    }
    let node = new NumberNode(token.start, token.end);
    node.value = token.value;
    ctx.t++; // consume number
    return node;
}

function getBinaryOpPrecedence(op: string): number {
    switch (op) {
        case "+":
        case "-":
            return 1;
        case "*":
        case "/":
            return 2;
        default:
            return 0;
    }
}

const recover = (ctx: Context) => {
 
    while (
        ctx.t < ctx.tokens.length && 
        ctx.tokens[ctx.t].name !== "EOS" && 
        ['let', 'if'].includes(ctx.tokens[ctx.t].name)
    ) {
        ctx.t++;
    }
}