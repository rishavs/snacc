{$ifdef windows}{$apptype console}{$endif}
{$mode objfpc}{$H+}{$J-}{$R+}
{$overflowchecks on}{$scopedenums on}
{$modeswitch advancedrecords}

program picsel;

uses
    SysUtils, 
    parser;

var
    text: string;
    StartTime, EndTime: TDateTime;
    ExecutionTime: Double;

begin
    // writeln('Parameter count: ', ParamCount);
    StartTime := Now;

    if ParamCount = 0 then begin
        writeln('Snacc Programming Language');
        writeln('Version 0.0.1');
        writeln('Usage: snacc [options] <file>');
        writeln('Options:');
        writeln('  -h, --help         Print this help message');
    end else if (ParamCount = 1) and (ParamStr(1) = 'run') then begin
        writeln('Please provide a tasty file to compile');
    end else if (ParamCount = 2) and (ParamStr(1) = 'run') then begin
        writeln('Compiling project with entrypoint file: ', ParamStr(2));
        parse_file(ParamStr(2));
    end else begin
        writeln('Invalid arguments');
        writeln('Only the "run" command is currently supported.');
    end;


    ExecutionTime := Now - StartTime;
    Writeln('Program Execution time: ', FormatFloat('0.0000000000000', ExecutionTime), ' seconds');

end.
