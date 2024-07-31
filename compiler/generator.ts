import { Node, RootNode, FunCallNode, IntNode, FloatNode, DeclarationNode, 
    IdentifierNode, GroupedExprNode, 
    BinaryNode,
    UnaryNode
} from "./ast";

export const genC99Files = async (ast: RootNode) => {
    let cMainFile = './oven/main.c';
    let cHeaderFile = './oven/main.h';

    let cMainContent = genC99MainSource(ast);
    let mainBytes = await Bun.write(cMainFile, cMainContent);
    let headerBytes = await Bun.write(cHeaderFile, `#include <stdio.h>\n`);
    console.log(`Wrote ${mainBytes} bytes to ${cMainFile}`);
    console.log(`Wrote ${headerBytes} bytes to ${cHeaderFile}`);
}

export const genC99MainSource = (ast: RootNode): string => {
    let indentLevel = 1;
    let indent = '    '.repeat(indentLevel); // Using 4 spaces for indentation
    let stmts = ast.statements.map(
        statement => `${indent}${genStatement(statement, indentLevel)};`
    ).join('\n');
    
    return /*c99*/`int main() {
${stmts}
    return 0;
}`
};

const genStatement = (statement: Node, indentLevel: number): string => {
    if (statement instanceof FunCallNode) {
        return genFunCall(statement, indentLevel);
    } else if (statement instanceof DeclarationNode) {
        return genDeclaration(statement, indentLevel);
    } else {
        throw new Error(`Codegen Error: Unknown statement type: ${statement}`);
    }
}

const genDeclaration = (decl: DeclarationNode, indentLevel: number): string => {
    return `int ${decl.id!.value} = ${genExpr(decl.expression!, indentLevel)}`;    
}

const genExpr = (expr: Node, indentLevel: number): string => {
    if (expr instanceof IntNode) {
        return genInt(expr);
    } else if (expr instanceof FloatNode) {
        return genFloat(expr);
    } else if (expr instanceof FunCallNode) {
        return genFunCall(expr, indentLevel);
    } else if (expr instanceof IdentifierNode) {
        return genIdentifier(expr);
    } else if (expr instanceof GroupedExprNode) {
        return genGroupedExpr(expr, indentLevel);

    } else if (expr instanceof BinaryNode) {
        return genBinaryExpr(expr, indentLevel);
    } else if (expr instanceof UnaryNode) {
        return genUnaryExpr(expr, indentLevel);

    } else {
        throw new Error(`Codegen Error: Unknown expression type: ${JSON.stringify(expr, null, 2)}`);
    }
}

const genBinaryExpr = (node: BinaryNode, indentLevel: number): string => {
    let left = genExpr(node.left, indentLevel);
    let right = genExpr(node.right, indentLevel);
    return `${left} ${node.operator} ${right}`;
}

const genUnaryExpr = (node: UnaryNode, indentLevel: number): string => {
    let right = genExpr(node.right, indentLevel);
    return `${node.operator}${right}`;
}

const genFunCall = (func: FunCallNode, indentLevel: number): string => {
    let id = func.id!.value;
    let args = func.expressions!.map(arg => genExpr(arg, indentLevel)).join(', ');
    return `${id}(${args})`;
}

const genIdentifier = (node: IdentifierNode): string => {
    return node.value as string;
}

const genInt = (node: IntNode): string => {
    return node.value as string;
}

const genFloat = (node: FloatNode): string => {
    return node.value as string;
}

const genGroupedExpr = (node: GroupedExprNode, indentLevel: number): string => {
    return `(${genExpr(node.expression, indentLevel)})`;
}