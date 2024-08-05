import { Context } from './defs'
import { BinaryNode, DeclarationNode, IdentifierNode, IntNode, Node, PrimaryTypeNode, RootNode, TypeChainExprNode } from './ast'
import { genC99Files } from './generator';
import { lexFile } from './lexer'
import { parseFile } from './parser';
import { parseFileToAss } from './parseToAss';
import { IdentifierLeaf } from './ass';

export const transpileFile = async (filepath: string) => {
    const file = Bun.file(filepath);
    if (! await file.exists()) {
        console.error("Error: File not found");
    }
    // Check if file is a seawitch file *.sea
    if (!filepath.endsWith(".sea")) {
        console.error("Error: Invalid file type. Seawitch files must have a .sea extension");
    }

    // Transpile file
    let ctx = new Context(filepath, await file.text())

    // Lexing
    lexFile(ctx)

    // Parsing
    if (ctx.errors.length > 0) {
        console.error("Error: Lexing failed");
        console.error(ctx.errors)

        return false
    }

    // parseFileToAss(ctx)
    // console.log(ctx.ass)

    parseFile(ctx)
    console.log(ctx.tokens)

    // printAST(ctx.root)
    walk(ctx.root, visitor)

    if (ctx.errors.length > 0) {
        console.error("Error: Parsing failed");
        console.error(ctx.errors)

        return false
    }

    genC99Files(ctx.root)

    console.log(ctx.errors)
    // console.log("AST", JSON.stringify(ctx.root, [
    //     'tokens', 'name',
    //     'kind', 'operator', 'value', 'isFloat', 'isQualified', 'isNewDeclaration', 
    //     'isPublic', 'isMutable', 
    //     'statements', 'left', 'right', 'identifier', 'expression', 'expressions', 'id',
    //     'types', 'declaredType'
    // ], 4))
    // console.log(ctx)
    return true
}

const printAST = (node: Node, depth = 0) => {
    const indent = '  '.repeat(depth);
    console.log(`${indent}${node.constructor.name}`);

    for (const key in node) {
        const value = (node as any)[key];
        if (key === 'parent') continue
        // if (value instanceof Node) {
        //     printAST(value, depth + 1);
        
        if (Array.isArray(value) && value.length > 0) {
            console.log(`${indent}${key}: [`);
            for (const item of value) {
                printAST(item, depth + 1);
            }
            console.log(`${indent}]`);
        } else if (value instanceof Node && value !== null) {
            console.log(`${indent}  ${key}:`);
            printAST(value, depth + 1);
        } else if(value) {
            console.log(`${indent}${key}: ${value}`);
        }
    }
};


// TODO - how to return something. how to handle errors
const walk = (node: Node, fn: Function, depth = 0) => {
    fn(node, depth);
    Object.keys(node).forEach(key => {
        const value = (node as any)[key];
        if (key === 'parent') return;

        if (value instanceof Node) {
            walk(value, fn, depth + 1);
        } else if (Array.isArray(value) && value.length > 0) {
            for (const item of value) {
                walk(item, fn, depth + 1);
            }
        } 
    });
}

const visitor = (node: Node, depth: number) => {
    let indent = '  '.repeat(depth) 
    if (node instanceof RootNode ) {
        console.log(indent + "RootNode")
    } else if (node instanceof DeclarationNode) {
        console.log(indent + `Declaration Node: ${node.id.value}`)
    } else if (node instanceof TypeChainExprNode) {
        console.log(indent + `TypeExprNode: ${node.operator}`, node.types)
    } else if (node instanceof PrimaryTypeNode) {
        console.log(indent + `PrimaryTypeNode: ${node.value}`)
    } else if (node instanceof BinaryNode) {
        console.log(indent + `BinaryNode: ${node.operator}`)
    } else if (node instanceof IntNode) {
        console.log(indent + `IntNode: ${node.value}`)
    } else if (node instanceof IdentifierNode) {
        console.log(indent + `Identifier: ${node.value}`)
    }
}