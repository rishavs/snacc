import { Node, RootNode, FunCallNode, IntNode, FloatNode, DeclarationNode, IdentifierNode } from "./ast";

export const genC99 = async (ast: RootNode) => {
    let cMainFile = './oven/main.c';
    let cHeaderFile = './oven/main.h';

    let cMainContent = genRoot(ast);
    let mainBytes = await Bun.write(cMainFile, cMainContent);
    let headerBytes = await Bun.write (cHeaderFile, `#include <stdio.h>\n`);
    console.log(`Wrote ${mainBytes} bytes to ${cMainFile}`);
    console.log(`Wrote ${headerBytes} bytes to ${cHeaderFile}`);
}

export const genRoot = (ast: RootNode): string => {
    let indentLevel = 1;
    let indent = '    '.repeat(indentLevel); // Using 4 spaces for indentation
    let stmts = ast.statements.map(
        statement => `${indent}${genStatement(statement, indentLevel)};`
    ).join('\n');
    
    return /*c99*/`
int main() {
${stmts}
    return 0;
}`
};

const genStatement = (statement: Node, indentLevel: number): string => {
    // Assuming you have different types of statements
    // You would check the type of the statement here and call the appropriate function
    // For example:
    switch (statement.kind) {
        case 'FUNCALL':
            return genFunCall(statement, indentLevel);
        case 'DECLARE':
            return genDeclaration(statement, indentLevel);
        default:
            throw new Error(`Codegen Error: Unknown statement type: ${statement.kind}`);
            // return '';
    }
}

const genDeclaration = (decl: DeclarationNode, indentLevel: number): string => {
    return `int ${decl.identifier!.value} = ${genExpr(decl.expression!, indentLevel)}`;    
}

const genExpr = (expr: Node, indentLevel: number): string => {
    // Assuming ExprNode is a union type of all possible expressions
    if (expr instanceof IntNode) {
        return genInt(expr);
    } else if (expr instanceof FloatNode) {
        return genFloat(expr);
    } else if (expr instanceof FunCallNode) {
        return genFunCall(expr, indentLevel);
    } else if (expr instanceof IdentifierNode) {
        return genIdentifier(expr);
    } else {
        throw new Error(`Codegen Error: Unknown expression type: ${expr}`);
    }
}

const genFunCall = (func: FunCallNode, indentLevel: number): string => {
    let id = func.id!.value
    let args = func.expressions!.map(arg => genExpr(arg, indentLevel)).join(', ');
    return `${id}(${args})`
}

const genIdentifier = (node: IdentifierNode): string => {
    return node.value as string
}

const genInt = (node: IntNode): string => {
    return node.value as string
}

const genFloat = (node: FloatNode): string => {
    return node.value as string
}