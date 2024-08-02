import { RootNode } from "./ast"


export const builtinTypes = {
    'Int': 'int64_t',
    'Dec': 'double',
    'Bool': 'bool',
    'Char': 'char', // int8_t
}

export const keywords = [
    'let', 'mut', 'pub', 'fn', 'if', 'else', 'while', 'for', 'return', 
    'break', 'continue', 'true', 'false', 'null', 'type'
] 

export const operators = [
    // 2 char Binary operators
    '==', '!=', '<=', '>=', '&&', '||',  

    // 1 char Binary operators
    '<', '>', '+', '-', '*', '/', '%',
    
    // 2 char Assignment operators
    '+=', '-=', '*=', '/=', '%=',
    
    // 1 char Assignment operators
    '=', 
    
    // Unary operators
    '!', '=',

    // 2 char Misc operators
    '->', '=>', 
    
    //  1 char Misc operators
    ':', '|', '&'
]

export class Token {
    name: string
    value?: string
    start: number
    end: number
    line : number
    constructor(name: string, start: number, end: number, line: number) {
        this.name = name
        this.start = start
        this.end = end
        this.line = line
    }
}

export class Context {
    filepath: string

    src: string = ""
    c: number = 0
    line : number = 0

    tokens : Token[] = []
    lexingDuration: number = 0

    t: number = 0
    root : RootNode = new RootNode(0, 0)
    parsingDuration: number = 0

    transpilingDuration: number = 0
    errors : Error[] = []

    constructor(filepath: string, src: string) {
        this.filepath = filepath
        this.src = src
    }
}