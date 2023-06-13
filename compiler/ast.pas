

    // type Node = record
    //     Kind        : NodeKind;
    //     Value       : string;
    //     Children    : array of Node;
    // end;



// type
//   TOperator = (OpAdd, OpSub, OpMul, OpDiv);
  
//   TExpressionNode = record
//     case NodeKind: (KindValue, KindBinaryOp) of
//       KindValue: (
//         Value: Integer;
//       );
//       KindBinaryOp: (
//         Operator: TOperator;
//         Left: ^TExpressionNode;
//         Right: ^TExpressionNode;
//       );
//   end;
  
//   TAssignmentNode = record
//     Identifier: string;
//     Expression: ^TExpressionNode;
//   end;
  
// var
//   xyzNode: TAssignmentNode;
//   expressionNode: TExpressionNode;

// begin
//   expressionNode.NodeKind := KindBinaryOp;
//   expressionNode.KindBinaryOp.Operator := OpAdd;
  
//   New(expressionNode.KindBinaryOp.Left);
//   expressionNode.KindBinaryOp.Left.NodeKind := KindValue;
//   expressionNode.KindBinaryOp.Left.Value := 1;
  
//   New(expressionNode.KindBinaryOp.Right);
//   expressionNode.KindBinaryOp.Right.NodeKind := KindValue;
//   expressionNode.KindBinaryOp.Right.Value := 2;
  
//   xyzNode.Identifier := 'xyz';
//   xyzNode.Expression := @expressionNode;
// end.
