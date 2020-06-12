unit apiDeprecated;

{$I apiConfig.inc}

interface

uses
  Windows;

type

  IAIMPDeprecatedTaskOwner = interface(IUnknown)
  ['{41494D50-5461-736B-4F77-6E6572000000}']
    function IsCanceled: LongBool;
  end;

  IAIMPDeprecatedTask = interface(IUnknown)
  ['{41494D50-5461-736B-0000-000000000000}']
    procedure Execute(Owner: IAIMPDeprecatedTaskOwner); stdcall;
  end;

  { IAIMPDeprecatedServiceSynchronizer }

  IAIMPDeprecatedServiceSynchronizer = interface(IUnknown)
  ['{41494D50-5372-7653-796E-637200000000}']
    function ExecuteInMainThread(Task: IAIMPDeprecatedTask; ExecuteNow: LongBool): HRESULT; stdcall;
  end;

  { IAIMPDeprecatedServiceThreadPool }

  IAIMPDeprecatedServiceThreadPool = interface(IUnknown)
  ['{41494D50-5372-7654-6872-64506F6F6C00}']
    function Cancel(TaskHandle: THandle; Flags: DWORD): HRESULT; stdcall;
    function Execute(Task: IAIMPDeprecatedTask; out TaskHandle: THandle): HRESULT; stdcall;
    function WaitFor(TaskHandle: THandle): HRESULT; stdcall;
  end;

implementation

end.
