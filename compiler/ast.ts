export class Node {
    start: number;
    line: number;

    end?: number;
    parent?: Node;
    constructor(start: number, line: number) {
        this.start = start;
        this.line = line;
    }
}

export class RootNode extends Node {
    statements: Node[] = []
    constructor(start: number, line: number) {
        super(start, line);
    }
}

export class FunCallNode extends Node {
    id!: IdentifierNode;
    expressions?: Node[];
    constructor(start: number, line: number) {
        super(start, line);
    }
}
// ---------------------------
// Statements
// ---------------------------
export class FunCallStmtNode extends Node {
    id?: IdentifierNode;
    expressions?: Node[];
    constructor(start: number, line: number) {
        super(start, line);
    }
}

export class DeclarationNode extends Node {
    isMutable?: boolean = false;
    isPublic?: boolean = false;
    isNewDeclaration: boolean = false;

    declaredType?: TypeChainExprNode;
    id!: IdentifierNode;
    expression?: Node;
    constructor(start: number, line: number) {
        super(start, line);
    }
}

// ---------------------------
// Type Expressions
// ---------------------------
export class PrimaryTypeNode extends Node {
    value: string;
    constructor(start: number, line: number, value: string) {
        super(start, line);
        this.value = value;
    }
}

export class TypeChainExprNode extends Node {
    operator: 'AND' | 'OR' = 'OR'
    types : string[] = []
    constructor(start: number, line: number) {
        super(start, line);
    }
}

// ---------------------------
// Expressions
// ---------------------------
export class UnaryNode extends Node {
    operator: string;
    right: Node;
    constructor(start: number, line: number, operator: string, right: Node) {
        super(start, line);
        this.operator = operator;
        this.right = right;
    }
}

export class BinaryNode extends Node {
    operator: string;
    left: Node;
    right: Node;
    constructor(start: number, line: number, operator: string, left: Node, right: Node) {
        super(start, line);
        this.operator = operator;
        this.left = left;
        this.right = right;
    }
}

export class GroupedExprNode extends Node {
    expression: Node;
    constructor(start: number, line: number, expr: Node) {
        super(start, line);
        this.expression = expr;
    }
}

export class IdentifierNode extends Node {
    isQualified: boolean = false
    value!: string;
    constructor(start: number, line: number, value: string) {
        super(start, line);
        this.value = value;
    }
}


export class IntNode extends Node {
    value: string;
    constructor(start: number, line: number, value: string) {
        super(start, line);
        this.value = value;
    }
}

export class FloatNode extends Node {
    value: string;
    constructor(start: number, line: number, value: string) {
        super(start, line);
        this.value = value;
    }
}
