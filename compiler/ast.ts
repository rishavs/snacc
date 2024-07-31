export class Node {
    kind: string
    start: number;
    line: number;

    end?: number;
    parent?: Node;
    constructor(kind: string, start: number, line: number) {
        this.kind = kind;
        this.start = start;
        this.line = line;
    }
}

export class RootNode extends Node {
    statements: Node[] = []
    constructor(start: number, line: number) {
        super('ROOT', start, line);
    }
}

export class FunCallNode extends Node {
    id?: IdentifierNode;
    expressions?: Node[];
    constructor(start: number, line: number) {
        super('FUNCALL', start, line);
    }
}
// ---------------------------
// Statements
// ---------------------------
export class FunCallStmtNode extends Node {
    id?: IdentifierNode;
    expressions?: Node[];
    constructor(start: number, line: number) {
        super('FUNCALL', start, line);
    }
}

export class DeclarationNode extends Node {
    isMutable?: boolean;
    isPublic?: boolean;
    isNewDeclaration?: boolean;
    identifier?: IdentifierNode;
    expression?: Node;
    constructor(start: number, line: number) {
        super('DECLARE', start, line);
    }
}

// ---------------------------
// Expressions
// ---------------------------
export class UnaryNode extends Node {
    operator?: string;
    right?: Node;
    constructor(start: number, line: number) {
        super('UNARY', start, line);
    }
}

export class BinaryNode extends Node {
    operator?: string;
    left?: Node;
    right?: Node;
    constructor(start: number, line: number) {
        super('BINARY', start, line);
    }
}

export class IdentifierNode extends Node {
    isQualified?: boolean;
    value?: string;
    constructor(start: number, line: number) {
        super('IDENTIFIER', start, line);
    }
}

export class NumberNode extends Node {
    value?: string;
    isFloat?: boolean;
    constructor(start: number, line: number) {
        super('NUMBER', start, line);
    }
}
export class IntNode extends Node {
    value?: string;
    constructor(start: number, line: number) {
        super('INT', start, line);
    }
}

export class FloatNode extends Node {
    value?: string;
    constructor(start: number, line: number) {
        super('FLOAT', start, line);
    }
}
