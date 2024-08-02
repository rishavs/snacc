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
    id!: IdentifierNode;
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
    isMutable?: boolean = false;
    isPublic?: boolean = false;
    isNewDeclaration: boolean = false;

    typeExpr?: BinaryExpressionTypeNode;
    id!: IdentifierNode;
    expression?: Node;
    constructor(start: number, line: number) {
        super('DECLARE', start, line);
    }
}

// ---------------------------
// Type Expressions
// ---------------------------
export class PrimaryTypeNode extends Node {
    value: string;
    constructor(start: number, line: number, value: string) {
        super('PRIMARYTYPE', start, line);
        this.value = value;
    }
}

export class ObjectTypeNode extends Node {
    properties!: Map<string, BinaryExpressionTypeNode>;
    constructor(start: number, line: number) {
        super('OBJECTTYPE', start, line);
    }
}

export class BinaryExpressionTypeNode extends Node {
    left!: PrimaryTypeNode | ObjectTypeNode | BinaryExpressionTypeNode;
    right?: PrimaryTypeNode | ObjectTypeNode | BinaryExpressionTypeNode;
    operator?: 'AND' | 'OR';
    constructor(start: number, line: number) {
        super('TYPEEXPRESSION', start, line);
        
    }
}

// ---------------------------
// Expressions
// ---------------------------
export class UnaryNode extends Node {
    operator: string;
    right: Node;
    constructor(start: number, line: number, operator: string, right: Node) {
        super('UNARY', start, line);
        this.operator = operator;
        this.right = right;
    }
}

export class BinaryNode extends Node {
    operator: string;
    left: Node;
    right: Node;
    constructor(start: number, line: number, operator: string, left: Node, right: Node) {
        super('BINARY', start, line);
        this.operator = operator;
        this.left = left;
        this.right = right;
    }
}

export class GroupedExprNode extends Node {
    expression: Node;
    constructor(start: number, line: number, expr: Node) {
        super('GROUPED', start, line);
        this.expression = expr;
    }
}

export class IdentifierNode extends Node {
    isQualified: boolean = false
    value!: string;
    constructor(start: number, line: number, value: string) {
        super('IDENTIFIER', start, line);
        this.value = value;
    }
}


export class IntNode extends Node {
    value: string;
    constructor(start: number, line: number, value: string) {
        super('INT', start, line);
        this.value = value;
    }
}

export class FloatNode extends Node {
    value: string;
    constructor(start: number, line: number, value: string) {
        super('FLOAT', start, line);
        this.value = value;
    }
}
