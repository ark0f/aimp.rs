{************************************************}
{*                                              *}
{*          AIMP Programming Interface          *}
{*               v4.60 build 2160               *}
{*                                              *}
{*                Artem Izmaylov                *}
{*                (C) 2006-2019                 *}
{*                 www.aimp.ru                  *}
{*            Mail: support@aimp.ru             *}
{*                                              *}
{************************************************}

unit apiThreading;

{$I apiConfig.inc}

interface

uses
  Windows;

const
  SID_IAIMPTask = '{41494D50-5461-736B-3200-000000000000}';
  IID_IAIMPTask: TGUID = SID_IAIMPTask;

  SID_IAIMPTaskOwner = '{41494D50-5461-736B-4F77-6E6572320000}';
  IID_IAIMPTaskOwner: TGUID = SID_IAIMPTaskOwner;
  
  SID_IAIMPTaskPriority = '{41494D50-5461-736B-5072-696F72697479}';
  IID_IAIMPTaskPriority: TGUID = SID_IAIMPTaskPriority;

  SID_IAIMPServiceThreads = '{41494D50-5372-7654-6872-656164730000}';
  IID_IAIMPServiceThreads: TGUID = SID_IAIMPServiceThreads;

  // Flags for IAIMPServiceThreads.Cancel and ExecuteInMainThread
  AIMP_SERVICE_THREADS_FLAGS_WAITFOR = $1;

  // IAIMPTaskPriority.GetPriority
  AIMP_TASK_PRIORITY_NORMAL = 0;
  AIMP_TASK_PRIORITY_LOW    = 1;
  AIMP_TASK_PRIORITY_HIGH   = 2;

type
  IAIMPTaskOwner = interface;

  { IAIMPTask }

  IAIMPTask = interface(IUnknown)
  [SID_IAIMPTask]
    procedure Execute(Owner: IAIMPTaskOwner); stdcall;
  end;
  
  { IAIMPTaskPriority }
  
  IAIMPTaskPriority = interface(IUnknown)
  [SID_IAIMPTaskPriority]
    function GetPriority: Integer; stdcall;
  end;

  { IAIMPTaskOwner }

  IAIMPTaskOwner = interface(IUnknown)
  [SID_IAIMPTaskOwner]
    function IsCanceled: LongBool; stdcall;
  end;

  { IAIMPServiceThreads }

  IAIMPServiceThreads = interface(IUnknown)
  [SID_IAIMPServiceThreads]
    function ExecuteInMainThread(Task: IAIMPTask; Flags: DWORD): HRESULT; stdcall;
    function ExecuteInThread(Task: IAIMPTask; out TaskHandle: THandle): HRESULT; stdcall;
    function Cancel(TaskHandle: THandle; Flags: DWORD): HRESULT; stdcall;
    function WaitFor(TaskHandle: THandle): HRESULT; stdcall;
  end;

implementation

end.
