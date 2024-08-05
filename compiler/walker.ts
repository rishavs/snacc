// Step-by-Step Plan
// Define the Node Structure: Define a base class for the nodes and specific classes for different node types.
// Define the Iterator Interface: Define an interface for the iterator.
// Implement the Concrete Iterator: Implement the iterator to traverse the AST.
// Implement the Traversal Logic: Use the iterator to traverse and print the AST.
// Implementation

// class Node {
//     start: number;
//     line: number;
//     parent?: Node;

//     constructor(start: number, line: number) {
//         this.start = start;
//         this.line = line;
//     }

//     accept(visitor: Visitor) {
//         visitor.visit(this);
//     }
// }

// class RootNode extends Node {
//     statements: Node[] = [];

//     constructor(start: number, line: number) {
//         super(start, line);
//     }

//     accept(visitor: Visitor) {
//         visitor.visitRootNode(this);
//         this.statements.forEach(stmt => stmt.accept(visitor));
//     }
// }

// class FunCallNode extends Node {
//     id: string;
//     expressions: Node[] = [];

//     constructor(start: number, line: number, id: string) {
//         super(start, line);
//         this.id = id;
//     }

//     accept(visitor: Visitor) {
//         visitor.visitFunCallNode(this);
//         this.expressions.forEach(expr => expr.accept(visitor));
//     }
// }

// class DeclarationNode extends Node {
//     id: string;
//     expression?: Node;

//     constructor(start: number, line: number, id: string) {
//         super(start, line);
//         this.id = id;
//     }

//     accept(visitor: Visitor) {
//         visitor.visitDeclarationNode(this);
//         this.expression?.accept(visitor);
//     }
// }

// class ASTIterator implements Iterator<Node> {
//     private stack: Node[] = [];

//     constructor(root: Node) {
//         this.stack.push(root);
//     }

//     next(): Node | null {
//         if (this.hasNext()) {
//             const currentNode = this.stack.pop()!;
//             if (currentNode instanceof RootNode) {
//                 for (let i = currentNode.statements.length - 1; i >= 0; i--) {
//                     this.stack.push(currentNode.statements[i]);
//                 }
//             } else if (currentNode instanceof FunCallNode) {
//                 for (let i = currentNode.expressions.length - 1; i >= 0; i--) {
//                     this.stack.push(currentNode.expressions[i]);
//                 }
//             } else if (currentNode instanceof DeclarationNode && currentNode.expression) {
//                 this.stack.push(currentNode.expression);
//             }
//             return currentNode;
//         }
//         return null;
//     }

//     hasNext(): boolean {
//         return this.stack.length > 0;
//     }
// }

// function printAST(root: Node) {
//     const iterator = new ASTIterator(root);
//     while (iterator.hasNext()) {
//         const node = iterator.next();
//         if (node instanceof RootNode) {
//             console.log("RootNode");
//         } else if (node instanceof FunCallNode) {
//             console.log(`FunCallNode with id: ${node.id}`);
//         } else if (node instanceof DeclarationNode) {
//             console.log(`DeclarationNode with id: ${node.id}`);
//         } else {
//             console.log("Unknown Node");
//         }
//     }
// }

// // Example usage
// const rootNode = new RootNode(0, 1);
// const funCallNode = new FunCallNode(5, 2, "foo");
// const declarationNode = new DeclarationNode(10, 3, "x");

// rootNode.statements.push(funCallNode);
// rootNode.statements.push(declarationNode);

// printAST(rootNode);