{$ifdef windows}{$apptype console}{$endif}
{$mode objfpc}{$H+}{$J-}{$R+}
{$overflowchecks on}{$scopedenums on}
{$modeswitch advancedrecords}

unit errors;

interface
    uses
        Generics.Collections,
        defs;
    
    type errorKind = (
        SyntaxError_NoStatementsFound
        , TypeError
        , ReferenceError        
        , RuntimeError
        , AssertionError
        , NotImplementedError
        , UnHandledError

    );
    
    type Error = record
            kind            : errorKind;

            id              : string;
            msg             : string;
            hint            : string;

            cursor          : integer;
            prevLineNum     : integer;
            currLineNum     : integer;
            nextLineNum     : integer;

            prevLineText    : string;
            currLineText    : string;
            nextLineText    : string;
        end;

    var 
        errorsList: array of Error;

    procedure addErrorsToList(errId: string; var fp: FileHandle; errPos:Integer; errLine: Integer);


implementation

    procedure addErrorsToList(errId: string; var fp: FileHandle; errPos:Integer; errLine: Integer);
    var
        err: Error;
        // lineBefore, lineAt, lineAfter: string;
    begin
        case errId of
        'SyntaxError:NoStatementsFound': begin
            err.id          := 'SyntaxError:NoStatementsFound';
            err.msg         := 'No statements found';
            err.hint        := 'Please check your code and try again';

        end;
        'SyntaxError:ExpectedStatement': begin
            err.id          := 'SyntaxError:ExpectedStatement';
            err.msg         := 'Expected statement';
            err.hint        := 'Please check your code and try again';

        end;
        end;

        err.cursor      := errPos;
        err.currLineNum := errLine;

        SetLength(errorsList, Length(errorsList) + 1);
        errorsList[High(errorsList)] := err;
        
    end;
    // function ReadLinesAroundPosition(var fp: TextFile; position: Int64; var lineBefore, lineAt, lineAfter: string): Boolean;
    // var
    //     currentPos: Integer;
    //     currentLine: string;
    // begin
    //     Result := False;
    //     lineBefore := '';
    //     lineAt := '';
    //     lineAfter := '';

    //     // Save the current position
    //     currentPos := FilePos(fp);

    //     // Start reading from the beginning of the file
    //     Reset(fp);

    //     try
    //         // Read lines until reaching the specified position
    //         while not Eof(fp) and (currentPos < position) do
    //         begin
    //         ReadLn(fp, currentLine);
    //         Inc(currentPos, Length(currentLine) + 2);  // Add 2 for CR+LF characters
    //         lineBefore := currentLine;
    //         end;

    //         // Read the line at the specified position
    //         if currentPos = position then
    //         begin
    //         ReadLn(fp, currentLine);
    //         lineAt := currentLine;
    //         Inc(currentPos, Length(currentLine) + 2);  // Add 2 for CR+LF characters
    //         end;

    //         // Read the line after the specified position
    //         if not Eof(fp) then
    //         begin
    //         ReadLn(fp, currentLine);
    //         lineAfter := currentLine;
    //         end;

    //         Result := True;
    //     finally
    //         // Restore the original file position
    //         Seek(fp, currentPos);
    //     end;
    // end;


end.