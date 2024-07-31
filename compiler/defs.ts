import { RootNode } from "./ast"

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