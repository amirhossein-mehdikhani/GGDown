import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  HardDrive,
  Download,
  Upload,
  Folder,
  File,
  FileText,
  Image as ImageIcon,
  Video,
  Archive,
  RefreshCw,
  Search,
  Plus,
  Play,
  Pause,
  Trash2,
  Settings,
  LogIn,
  LogOut,
  ChevronRight,
  CheckCircle,
  AlertCircle,
  ArrowDownCircle,
  ArrowUpCircle,
  WifiOff,
  ExternalLink,
  Copy,
  Check
} from "lucide-react";

interface DriveFile {
  id: string;
  name: string;
  mimeType: string;
  size?: string;
  modifiedTime?: string;
  iconLink?: string;
  is_folder: boolean;
}

interface TransferTask {
  id: string;
  transfer_type: "download" | "upload";
  file_name: string;
  file_id?: string;
  local_path: string;
  total_bytes: number;
  transferred_bytes: number;
  status: "pending" | "active" | "paused" | "completed" | "error";
  speed_bytes_per_sec: number;
  error_message?: string;
}

interface UserData {
  user_email?: string;
  user_name?: string;
  user_picture?: string;
}

interface AuthStatus {
  is_logged_in: boolean;
  has_credentials: boolean;
  client_id?: string;
  user?: UserData;
}

function formatBytes(bytes: number, decimals = 2) {
  if (bytes === 0) return "0 B";
  const k = 1024;
  const dm = decimals < 0 ? 0 : decimals;
  const sizes = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(dm)) + " " + sizes[i];
}

function formatSpeed(bytesPerSec: number) {
  return `${formatBytes(bytesPerSec)}/s`;
}

export default function App() {
  const [activeTab, setActiveTab] = useState<"drive" | "transfers">("drive");
  const [authStatus, setAuthStatus] = useState<AuthStatus | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [clientId, setClientId] = useState("");
  const [clientSecret, setClientSecret] = useState("");
  const [authLoading, setAuthLoading] = useState(false);
  const [pendingAuthUrl, setPendingAuthUrl] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  // Drive state
  const [files, setFiles] = useState<DriveFile[]>([]);
  const [currentFolderId, setCurrentFolderId] = useState<string | null>(null);
  const [breadcrumbs, setBreadcrumbs] = useState<Array<{ id: string | null; name: string }>>([
    { id: null, name: "My Drive" },
  ]);
  const [searchQuery, setSearchQuery] = useState("");
  const [driveLoading, setDriveLoading] = useState(false);
  const [driveError, setDriveError] = useState<string | null>(null);
  const [newFolderName, setNewFolderName] = useState("");
  const [showNewFolderModal, setShowNewFolderModal] = useState(false);

  // Transfers state
  const [transfers, setTransfers] = useState<TransferTask[]>([]);
  const [transferFilter, setTransferFilter] = useState<"all" | "active" | "completed">("all");
  const [isOnline, setIsOnline] = useState(navigator.onLine);

  // Monitor network connectivity
  useEffect(() => {
    const handleOnline = () => {
      setIsOnline(true);
      autoResumeTransfers();
    };
    const handleOffline = () => setIsOnline(false);

    window.addEventListener("online", handleOnline);
    window.addEventListener("offline", handleOffline);

    return () => {
      window.removeEventListener("online", handleOnline);
      window.removeEventListener("offline", handleOffline);
    };
  }, [transfers]);

  // Load Auth status & Transfers initially
  useEffect(() => {
    loadAuthStatus();
    loadTransfers();

    const unlistenProgress = listen<any>("transfer-progress", (event) => {
      const payload = event.payload;
      setTransfers((prev) =>
        prev.map((t) =>
          t.id === payload.id
            ? {
                ...t,
                transferred_bytes: payload.transferred_bytes,
                total_bytes: payload.total_bytes,
                speed_bytes_per_sec: payload.speed_bytes_per_sec,
                status: payload.status,
                error_message: payload.error_message,
              }
            : t
        )
      );
    });

    const unlistenRemoved = listen<string>("transfer-removed", (event) => {
      const id = event.payload;
      setTransfers((prev) => prev.filter((t) => t.id !== id));
    });

    return () => {
      unlistenProgress.then((f) => f());
      unlistenRemoved.then((f) => f());
    };
  }, []);

  // When auth changes and user is logged in, load drive files
  useEffect(() => {
    if (authStatus?.is_logged_in) {
      loadDriveFiles(currentFolderId);
    }
  }, [authStatus?.is_logged_in, currentFolderId]);

  const loadAuthStatus = async () => {
    try {
      const status: AuthStatus = await invoke("auth_get_status");
      setAuthStatus(status);
      if (status.client_id) {
        setClientId(status.client_id);
      }
    } catch (e) {
      console.error("Failed to load auth status:", e);
    }
  };

  const loadTransfers = async () => {
    try {
      const list: TransferTask[] = await invoke("transfer_list");
      setTransfers(list);
    } catch (e) {
      console.error("Failed to load transfers:", e);
    }
  };

  const autoResumeTransfers = async () => {
    for (const t of transfers) {
      if (t.status === "error" || t.status === "pending") {
        try {
          await invoke("transfer_start", { taskId: t.id });
        } catch (e) {
          console.warn(`Auto resume failed for ${t.file_name}:`, e);
        }
      }
    }
  };

  const handleSaveCredentials = async () => {
    if (!clientId.trim() || !clientSecret.trim()) {
      alert("Please enter both Client ID and Client Secret.");
      return;
    }
    try {
      await invoke("auth_save_credentials", {
        clientId: clientId.trim(),
        clientSecret: clientSecret.trim(),
      });
      await loadAuthStatus();
      alert("Credentials saved. Now click 'Sign In with Google'.");
    } catch (e) {
      alert(`Error saving credentials: ${e}`);
    }
  };

  const handleStartLogin = async () => {
    if (!clientId.trim() || !clientSecret.trim()) {
      alert("Please enter and save your Client ID and Client Secret first.");
      return;
    }

    setAuthLoading(true);
    try {
      await invoke("auth_save_credentials", {
        clientId: clientId.trim(),
        clientSecret: clientSecret.trim(),
      });

      const authUrl: string = await invoke("auth_start_login");
      setPendingAuthUrl(authUrl);

      try {
        await invoke("open_browser_url", { url: authUrl });
      } catch (err) {
        console.warn("Direct browser launch returned:", err);
      }

      // Check periodically if token was acquired
      const interval = setInterval(async () => {
        const status: AuthStatus = await invoke("auth_get_status");
        if (status.is_logged_in) {
          setAuthStatus(status);
          setShowSettings(false);
          setAuthLoading(false);
          setPendingAuthUrl(null);
          clearInterval(interval);
        }
      }, 1500);

      setTimeout(() => {
        clearInterval(interval);
        setAuthLoading(false);
      }, 180000);
    } catch (e) {
      alert(`Error starting login: ${e}`);
      setAuthLoading(false);
    }
  };

  const handleManualOpenBrowser = async () => {
    if (pendingAuthUrl) {
      try {
        await invoke("open_browser_url", { url: pendingAuthUrl });
      } catch (e) {
        console.error(e);
      }
    }
  };

  const handleCopyAuthUrl = () => {
    if (pendingAuthUrl) {
      navigator.clipboard.writeText(pendingAuthUrl);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    }
  };

  const handleLogout = async () => {
    try {
      await invoke("auth_logout");
      await loadAuthStatus();
      setFiles([]);
      setDriveError(null);
    } catch (e) {
      console.error(e);
    }
  };

  const loadDriveFiles = async (folderId: string | null = null, search?: string) => {
    setDriveLoading(true);
    setDriveError(null);
    try {
      const result: DriveFile[] = await invoke("drive_list", {
        folderId: folderId || undefined,
        search: search || undefined,
      });
      setFiles(result);
    } catch (e: any) {
      console.error("Error loading drive files:", e);
      setDriveError(typeof e === "string" ? e : (e?.message || "Error loading Drive files"));
    } finally {
      setDriveLoading(false);
    }
  };

  const handleFolderClick = (folder: DriveFile) => {
    setCurrentFolderId(folder.id);
    setBreadcrumbs((prev) => [...prev, { id: folder.id, name: folder.name }]);
  };

  const handleBreadcrumbClick = (index: number) => {
    const target = breadcrumbs[index];
    setBreadcrumbs((prev) => prev.slice(0, index + 1));
    setCurrentFolderId(target.id);
  };

  const handleCreateFolder = async () => {
    if (!newFolderName.trim()) return;
    try {
      await invoke("drive_create_folder", {
        name: newFolderName.trim(),
        parentId: currentFolderId || undefined,
      });
      setNewFolderName("");
      setShowNewFolderModal(false);
      loadDriveFiles(currentFolderId);
    } catch (e) {
      alert(`Error creating folder: ${e}`);
    }
  };

  const handleDeleteFile = async (file: DriveFile) => {
    if (!confirm(`Are you sure you want to delete "${file.name}"?`)) return;
    try {
      await invoke("drive_delete", { fileId: file.id });
      loadDriveFiles(currentFolderId);
    } catch (e) {
      alert(`Error deleting file: ${e}`);
    }
  };

  const handleDownloadFile = async (file: DriveFile) => {
    try {
      const size = file.size ? parseInt(file.size, 10) : 0;
      const task: TransferTask = await invoke("transfer_add_download", {
        fileId: file.id,
        fileName: file.name,
        fileSize: size,
        destinationDir: null,
      });

      setTransfers((prev) => [...prev, task]);
      await invoke("transfer_start", { taskId: task.id });
      setActiveTab("transfers");
    } catch (e) {
      alert(`Error starting download: ${e}`);
    }
  };

  const handleUploadClick = async () => {
    try {
      const paths: string[] = await invoke("system_pick_files");
      if (!paths || paths.length === 0) return;

      for (const filePath of paths) {
        try {
          const task: TransferTask = await invoke("transfer_add_upload", {
            filePath,
            parentId: currentFolderId || undefined,
          });

          setTransfers((prev) => [...prev, { ...task, status: "active" }]);
          await invoke("transfer_start", { taskId: task.id });
        } catch (err) {
          alert(`Error uploading ${filePath}: ${err}`);
        }
      }
      setActiveTab("transfers");
    } catch (e) {
      console.error("File selection error:", e);
    }
  };

  const handlePauseTask = async (taskId: string) => {
    try {
      setTransfers((prev) =>
        prev.map((t) => (t.id === taskId ? { ...t, status: "paused", speed_bytes_per_sec: 0 } : t))
      );
      await invoke("transfer_pause", { taskId });
    } catch (e) {
      console.error(e);
    }
  };

  const handleResumeTask = async (taskId: string) => {
    try {
      setTransfers((prev) =>
        prev.map((t) => (t.id === taskId ? { ...t, status: "active", error_message: undefined } : t))
      );
      await invoke("transfer_start", { taskId });
    } catch (e) {
      console.error(e);
    }
  };

  const handleCancelTask = async (taskId: string) => {
    try {
      await invoke("transfer_cancel", { taskId });
    } catch (e) {
      console.error(e);
    }
  };

  const getFileIcon = (file: DriveFile) => {
    if (file.is_folder) return <Folder className="w-5 h-5 text-amber-400 shrink-0" />;
    const mime = file.mimeType.toLowerCase();
    if (mime.includes("image")) return <ImageIcon className="w-5 h-5 text-emerald-400 shrink-0" />;
    if (mime.includes("video")) return <Video className="w-5 h-5 text-rose-400 shrink-0" />;
    if (mime.includes("zip") || mime.includes("tar") || mime.includes("compressed"))
      return <Archive className="w-5 h-5 text-purple-400 shrink-0" />;
    if (mime.includes("pdf") || mime.includes("document") || mime.includes("text"))
      return <FileText className="w-5 h-5 text-blue-400 shrink-0" />;
    return <File className="w-5 h-5 text-slate-400 shrink-0" />;
  };

  const activeTransfers = transfers.filter((t) => t.status === "active");
  const totalSpeed = activeTransfers.reduce((acc, cur) => acc + cur.speed_bytes_per_sec, 0);

  const filteredTransfers = transfers.filter((t) => {
    if (transferFilter === "active") return t.status === "active" || t.status === "pending";
    if (transferFilter === "completed") return t.status === "completed";
    return true;
  });

  return (
    <div className="flex flex-col h-screen w-screen bg-[#0b0f19] text-slate-100 overflow-hidden font-sans select-none">
      {/* Top Header Navbar */}
      <header className="h-16 bg-[#111827]/90 border-b border-slate-800/90 flex items-center justify-between px-5 shrink-0 backdrop-blur-md z-20">
        {/* App Logo & Branding */}
        <div className="flex items-center gap-3">
          <div className="w-9 h-9 rounded-xl bg-gradient-to-tr from-blue-600 to-indigo-500 flex items-center justify-center font-black text-white shadow-lg shadow-blue-500/25 text-sm tracking-wider">
            GD
          </div>
          <div className="flex flex-col">
            <div className="flex items-center gap-2">
              <span className="font-bold text-base tracking-tight text-white">GGDown</span>
              <span className="text-[10px] px-2 py-0.5 rounded-full bg-blue-500/10 text-blue-400 border border-blue-500/20 font-medium">
                Drive Manager
              </span>
            </div>
            <span className="text-[11px] text-slate-400">Resumable Cloud Download & Upload</span>
          </div>
        </div>

        {/* Navigation Tabs */}
        <div className="flex items-center gap-1.5 bg-[#0b0f19] p-1.5 rounded-xl border border-slate-800">
          <button
            onClick={() => setActiveTab("drive")}
            className={`flex items-center gap-2 px-4 py-2 rounded-lg text-xs font-semibold transition-all duration-200 ${
              activeTab === "drive"
                ? "bg-blue-600 text-white shadow-md shadow-blue-600/30"
                : "text-slate-400 hover:text-slate-200 hover:bg-slate-800/50"
            }`}
          >
            <HardDrive className="w-4 h-4" />
            Google Drive
          </button>
          <button
            onClick={() => setActiveTab("transfers")}
            className={`flex items-center gap-2 px-4 py-2 rounded-lg text-xs font-semibold transition-all duration-200 relative ${
              activeTab === "transfers"
                ? "bg-blue-600 text-white shadow-md shadow-blue-600/30"
                : "text-slate-400 hover:text-slate-200 hover:bg-slate-800/50"
            }`}
          >
            <Download className="w-4 h-4" />
            Transfers
            {activeTransfers.length > 0 && (
              <span className="bg-emerald-500 text-slate-950 text-[10px] font-black px-1.5 py-0.5 rounded-full animate-pulse">
                {activeTransfers.length}
              </span>
            )}
          </button>
        </div>

        {/* Right Status Controls */}
        <div className="flex items-center gap-3">
          {/* Internet Status */}
          <div
            className={`flex items-center gap-1.5 text-xs px-3 py-1.5 rounded-full border transition-all ${
              isOnline
                ? "bg-emerald-950/40 text-emerald-400 border-emerald-800/50"
                : "bg-rose-950/50 text-rose-400 border-rose-800/60 animate-pulse"
            }`}
          >
            {isOnline ? (
              <>
                <span className="w-2 h-2 rounded-full bg-emerald-400 shadow-sm shadow-emerald-400/50" />
                <span className="font-medium text-[11px]">Online</span>
              </>
            ) : (
              <>
                <WifiOff className="w-3.5 h-3.5" />
                <span className="font-medium text-[11px]">Offline (Waiting)</span>
              </>
            )}
          </div>

          {/* User Profile or Connect Button */}
          {authStatus?.is_logged_in ? (
            <div className="flex items-center gap-2 bg-slate-900/80 px-3 py-1.5 rounded-xl border border-slate-800">
              {authStatus.user?.user_picture ? (
                <img
                  src={authStatus.user.user_picture}
                  alt="avatar"
                  className="w-6 h-6 rounded-full border border-slate-700"
                />
              ) : (
                <div className="w-6 h-6 rounded-full bg-blue-600 text-[11px] flex items-center justify-center font-bold">
                  {authStatus.user?.user_name?.[0] || "U"}
                </div>
              )}
              <span className="text-xs text-slate-200 font-medium max-w-[140px] truncate">
                {authStatus.user?.user_email || "Connected"}
              </span>
              <button
                onClick={handleLogout}
                title="Sign Out"
                className="p-1 rounded text-slate-400 hover:text-rose-400 hover:bg-slate-800 transition"
              >
                <LogOut className="w-3.5 h-3.5" />
              </button>
            </div>
          ) : (
            <button
              onClick={() => setShowSettings(true)}
              className="flex items-center gap-1.5 bg-gradient-to-r from-blue-600 to-indigo-600 hover:from-blue-500 hover:to-indigo-500 text-white text-xs px-3.5 py-2 rounded-xl font-medium shadow-md shadow-blue-600/25 transition-all"
            >
              <LogIn className="w-3.5 h-3.5" />
              Connect Google Account
            </button>
          )}

          {/* Settings Button */}
          <button
            onClick={() => setShowSettings(true)}
            className="p-2 rounded-xl text-slate-400 hover:text-slate-100 bg-slate-900 hover:bg-slate-800 border border-slate-800 transition shadow-sm"
            title="Settings"
          >
            <Settings className="w-4 h-4" />
          </button>
        </div>
      </header>

      {/* Main Container */}
      <main className="flex-1 flex flex-col overflow-hidden p-5">
        {activeTab === "drive" ? (
          /* ================= DRIVE EXPLORER VIEW ================= */
          <div className="flex-1 flex flex-col overflow-hidden">
            {!authStatus?.is_logged_in ? (
              <div className="flex-1 flex flex-col items-center justify-center text-center p-8 bg-[#111827]/40 rounded-2xl border border-slate-800/80 shadow-xl">
                <div className="w-20 h-20 rounded-3xl bg-blue-500/10 border border-blue-500/20 flex items-center justify-center mb-5 text-blue-400 shadow-lg shadow-blue-500/10">
                  <HardDrive className="w-10 h-10" />
                </div>
                <h2 className="text-xl font-bold mb-2 text-white">Google Drive Not Connected</h2>
                <p className="text-slate-400 max-w-lg text-sm mb-6 leading-relaxed">
                  Connect your Google account to explore your Drive files, create folders, upload files with auto-resume on network failure, and perform high-speed chunked downloads.
                </p>
                <button
                  onClick={() => setShowSettings(true)}
                  className="bg-blue-600 hover:bg-blue-500 text-white font-semibold px-6 py-2.5 rounded-xl shadow-lg shadow-blue-600/25 transition-all flex items-center gap-2 text-sm"
                >
                  <Settings className="w-4 h-4" />
                  Configure & Connect Account
                </button>
              </div>
            ) : (
              <div className="flex-1 flex flex-col overflow-hidden">
                {/* Explorer Toolbar */}
                <div className="flex items-center justify-between mb-4 bg-[#111827]/80 p-3 rounded-2xl border border-slate-800/90 shrink-0">
                  {/* Breadcrumb Path */}
                  <div className="flex items-center gap-1.5 text-xs overflow-x-auto py-1">
                    {breadcrumbs.map((crumb, idx) => (
                      <div key={crumb.id || "root"} className="flex items-center gap-1.5">
                        {idx > 0 && <ChevronRight className="w-3.5 h-3.5 text-slate-500" />}
                        <button
                          onClick={() => handleBreadcrumbClick(idx)}
                          className={`hover:text-blue-400 transition font-medium px-2 py-1 rounded-lg ${
                            idx === breadcrumbs.length - 1
                              ? "text-blue-400 bg-blue-500/10 border border-blue-500/20"
                              : "text-slate-400 hover:bg-slate-800"
                          }`}
                        >
                          {crumb.name}
                        </button>
                      </div>
                    ))}
                  </div>

                  {/* Actions & Search */}
                  <div className="flex items-center gap-2.5">
                    <div className="relative">
                      <Search className="w-4 h-4 absolute left-3 top-2.5 text-slate-500" />
                      <input
                        type="text"
                        placeholder="Search in Drive..."
                        value={searchQuery}
                        onChange={(e) => setSearchQuery(e.target.value)}
                        onKeyDown={(e) => {
                          if (e.key === "Enter") loadDriveFiles(currentFolderId, searchQuery);
                        }}
                        className="bg-[#0b0f19] border border-slate-800 rounded-xl pl-9 pr-3.5 py-1.5 text-xs text-slate-200 placeholder-slate-500 focus:outline-none focus:border-blue-500 w-52 transition"
                      />
                    </div>

                    <button
                      onClick={() => loadDriveFiles(currentFolderId, searchQuery)}
                      title="Refresh"
                      className="p-2 bg-slate-900 hover:bg-slate-800 rounded-xl text-slate-300 border border-slate-800 transition"
                    >
                      <RefreshCw className={`w-4 h-4 ${driveLoading ? "animate-spin text-blue-400" : ""}`} />
                    </button>

                    <button
                      onClick={() => setShowNewFolderModal(true)}
                      className="flex items-center gap-1.5 bg-slate-900 hover:bg-slate-800 text-slate-200 px-3.5 py-2 rounded-xl text-xs font-medium border border-slate-800 transition"
                    >
                      <Plus className="w-3.5 h-3.5" />
                      New Folder
                    </button>

                    <button
                      onClick={handleUploadClick}
                      className="flex items-center gap-1.5 bg-blue-600 hover:bg-blue-500 text-white px-4 py-2 rounded-xl text-xs font-semibold shadow-md shadow-blue-600/20 transition cursor-pointer"
                    >
                      <Upload className="w-3.5 h-3.5" />
                      Upload File
                    </button>
                  </div>
                </div>

                {/* Drive File List Table */}
                <div className="flex-1 overflow-auto rounded-2xl border border-slate-800/90 bg-[#111827]/40 shadow-xl">
                  {driveLoading ? (
                    <div className="h-72 flex flex-col items-center justify-center text-slate-400 gap-3">
                      <RefreshCw className="w-7 h-7 animate-spin text-blue-500" />
                      <span className="text-sm font-medium">Fetching files from Google Drive...</span>
                    </div>
                  ) : driveError ? (
                    <div className="h-72 flex flex-col items-center justify-center text-center p-6 gap-3">
                      <div className="w-14 h-14 rounded-2xl bg-amber-500/10 border border-amber-500/20 flex items-center justify-center text-amber-400">
                        <AlertCircle className="w-7 h-7" />
                      </div>
                      <h4 className="text-base font-bold text-slate-100">Google Drive Permission Required</h4>
                      <p className="text-xs text-slate-400 max-w-lg leading-relaxed">{driveError}</p>
                      <button
                        onClick={async () => {
                          await handleLogout();
                          setShowSettings(true);
                        }}
                        className="mt-2 bg-gradient-to-r from-blue-600 to-indigo-600 hover:from-blue-500 hover:to-indigo-500 text-white text-xs font-semibold px-5 py-2.5 rounded-xl shadow-lg shadow-blue-600/25 transition"
                      >
                        Sign Out & Reconnect (Check Drive Permission Box)
                      </button>
                    </div>
                  ) : files.length === 0 ? (
                    <div className="h-72 flex flex-col items-center justify-center text-slate-500 gap-2">
                      <Folder className="w-12 h-12 text-slate-700" />
                      <span className="text-sm">This folder is empty</span>
                    </div>
                  ) : (
                    <table className="w-full text-left text-sm">
                      <thead className="bg-[#111827] text-slate-400 text-xs border-b border-slate-800 sticky top-0 backdrop-blur z-10">
                        <tr>
                          <th className="py-3 px-5 font-semibold">Name</th>
                          <th className="py-3 px-5 font-semibold w-36">Size</th>
                          <th className="py-3 px-5 font-semibold w-48">Last Modified</th>
                          <th className="py-3 px-5 font-semibold w-32 text-center">Actions</th>
                        </tr>
                      </thead>
                      <tbody className="divide-y divide-slate-800/60">
                        {files.map((file) => (
                          <tr
                            key={file.id}
                            className="hover:bg-slate-800/40 transition group cursor-pointer"
                            onDoubleClick={() => file.is_folder && handleFolderClick(file)}
                          >
                            <td className="py-3 px-5 flex items-center gap-3">
                              {getFileIcon(file)}
                              <span
                                onClick={() => file.is_folder && handleFolderClick(file)}
                                className={`font-medium truncate max-w-lg ${
                                  file.is_folder
                                    ? "text-blue-300 hover:text-blue-200 cursor-pointer"
                                    : "text-slate-200"
                                }`}
                              >
                                {file.name}
                              </span>
                            </td>
                            <td className="py-3 px-5 text-xs text-slate-400 font-mono">
                              {file.is_folder
                                ? "—"
                                : file.size
                                ? formatBytes(parseInt(file.size, 10))
                                : "—"}
                            </td>
                            <td className="py-3 px-5 text-xs text-slate-400">
                              {file.modifiedTime
                                ? new Date(file.modifiedTime).toLocaleDateString()
                                : "—"}
                            </td>
                            <td className="py-3 px-5 text-center">
                              <div className="flex items-center justify-center gap-2 opacity-80 group-hover:opacity-100">
                                {!file.is_folder && (
                                  <button
                                    onClick={() => handleDownloadFile(file)}
                                    title="Download (Resumable)"
                                    className="p-1.5 rounded-lg bg-blue-600/20 hover:bg-blue-600 text-blue-400 hover:text-white transition"
                                  >
                                    <Download className="w-4 h-4" />
                                  </button>
                                )}
                                <button
                                  onClick={() => handleDeleteFile(file)}
                                  title="Delete"
                                  className="p-1.5 rounded-lg bg-rose-600/20 hover:bg-rose-600 text-rose-400 hover:text-white transition"
                                >
                                  <Trash2 className="w-4 h-4" />
                                </button>
                              </div>
                            </td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  )}
                </div>
              </div>
            )}
          </div>
        ) : (
          /* ================= TRANSFERS MANAGER VIEW ================= */
          <div className="flex-1 flex flex-col overflow-hidden">
            {/* Transfers Header & Stats */}
            <div className="flex items-center justify-between mb-4 bg-[#111827]/80 p-3.5 rounded-2xl border border-slate-800/90 shrink-0">
              <div className="flex items-center gap-2">
                <button
                  onClick={() => setTransferFilter("all")}
                  className={`px-4 py-1.5 rounded-xl text-xs font-semibold transition ${
                    transferFilter === "all"
                      ? "bg-blue-600 text-white shadow-md shadow-blue-600/20"
                      : "bg-slate-900 text-slate-400 hover:text-slate-200 border border-slate-800"
                  }`}
                >
                  All ({transfers.length})
                </button>
                <button
                  onClick={() => setTransferFilter("active")}
                  className={`px-4 py-1.5 rounded-xl text-xs font-semibold transition ${
                    transferFilter === "active"
                      ? "bg-blue-600 text-white shadow-md shadow-blue-600/20"
                      : "bg-slate-900 text-slate-400 hover:text-slate-200 border border-slate-800"
                  }`}
                >
                  Active ({activeTransfers.length})
                </button>
                <button
                  onClick={() => setTransferFilter("completed")}
                  className={`px-4 py-1.5 rounded-xl text-xs font-semibold transition ${
                    transferFilter === "completed"
                      ? "bg-blue-600 text-white shadow-md shadow-blue-600/20"
                      : "bg-slate-900 text-slate-400 hover:text-slate-200 border border-slate-800"
                  }`}
                >
                  Completed ({transfers.filter((t) => t.status === "completed").length})
                </button>
              </div>

              {/* Total Speed Display */}
              <div className="flex items-center gap-2 bg-emerald-950/40 border border-emerald-800/50 px-3.5 py-1.5 rounded-xl text-emerald-400 text-xs font-bold">
                <ArrowDownCircle className="w-4 h-4 animate-bounce" />
                <span>Speed: {formatSpeed(totalSpeed)}</span>
              </div>
            </div>

            {/* Transfers Cards */}
            <div className="flex-1 overflow-auto space-y-3">
              {filteredTransfers.length === 0 ? (
                <div className="h-72 flex flex-col items-center justify-center text-slate-600 gap-3">
                  <Download className="w-12 h-12 text-slate-700" />
                  <span className="text-sm font-medium">No transfers in this view</span>
                </div>
              ) : (
                filteredTransfers.map((task) => {
                  const percent =
                    task.total_bytes > 0
                      ? Math.min(100, Math.round((task.transferred_bytes / task.total_bytes) * 100))
                      : 0;

                  return (
                    <div
                      key={task.id}
                      className="bg-[#111827]/90 border border-slate-800/90 rounded-2xl p-4 hover:border-slate-700/80 transition-all shadow-md"
                    >
                      <div className="flex items-center justify-between mb-3">
                        <div className="flex items-center gap-3 min-w-0">
                          {task.transfer_type === "download" ? (
                            <div className="w-9 h-9 rounded-xl bg-blue-500/10 border border-blue-500/20 flex items-center justify-center text-blue-400 shrink-0">
                              <ArrowDownCircle className="w-5 h-5" />
                            </div>
                          ) : (
                            <div className="w-9 h-9 rounded-xl bg-emerald-500/10 border border-emerald-500/20 flex items-center justify-center text-emerald-400 shrink-0">
                              <ArrowUpCircle className="w-5 h-5" />
                            </div>
                          )}
                          <div className="min-w-0">
                            <h4 className="text-sm font-bold truncate text-slate-100">
                              {task.file_name}
                            </h4>
                            <p className="text-xs text-slate-500 truncate font-mono">
                              {task.local_path}
                            </p>
                          </div>
                        </div>

                        {/* Status Badges & Action Buttons */}
                        <div className="flex items-center gap-2.5 shrink-0">
                          {(task.status === "active" || task.status === "pending") && (
                            <span className="text-xs px-3 py-1 rounded-full bg-emerald-950/60 text-emerald-400 border border-emerald-800/60 flex items-center gap-1.5 font-medium">
                              <span className="w-2 h-2 rounded-full bg-emerald-400 animate-ping" />
                              {task.speed_bytes_per_sec > 0 ? formatSpeed(task.speed_bytes_per_sec) : "Connecting..."}
                            </span>
                          )}
                          {task.status === "paused" && (
                            <span className="text-xs px-3 py-1 rounded-full bg-amber-950/50 text-amber-400 border border-amber-800/50 font-medium">
                              Paused
                            </span>
                          )}
                          {task.status === "completed" && (
                            <span className="text-xs px-3 py-1 rounded-full bg-blue-950/50 text-blue-400 border border-blue-800/50 flex items-center gap-1.5 font-medium">
                              <CheckCircle className="w-3.5 h-3.5" />
                              Completed
                            </span>
                          )}
                          {task.status === "error" && (
                            <span
                              className="text-xs px-3 py-1 rounded-full bg-rose-950/60 text-rose-400 border border-rose-800/60 flex items-center gap-1.5 font-medium"
                              title={task.error_message || "Error"}
                            >
                              <AlertCircle className="w-3.5 h-3.5" />
                              Interrupted (Ready to resume)
                            </span>
                          )}

                          {/* Control Buttons */}
                          {task.status === "active" || task.status === "pending" ? (
                            <button
                              onClick={() => handlePauseTask(task.id)}
                              title="Pause"
                              className="p-2 rounded-xl bg-slate-800 hover:bg-slate-700 text-slate-300 border border-slate-700/60 transition"
                            >
                              <Pause className="w-4 h-4" />
                            </button>
                          ) : task.status !== "completed" ? (
                            <button
                              onClick={() => handleResumeTask(task.id)}
                              title="Resume"
                              className="p-2 rounded-xl bg-blue-600 hover:bg-blue-500 text-white transition shadow-md shadow-blue-600/25"
                            >
                              <Play className="w-4 h-4" />
                            </button>
                          ) : null}

                          <button
                            onClick={() => handleCancelTask(task.id)}
                            title="Cancel & Remove"
                            className="p-2 rounded-xl bg-slate-900 hover:bg-rose-900/40 text-slate-400 hover:text-rose-400 border border-slate-800 transition"
                          >
                            <Trash2 className="w-4 h-4" />
                          </button>
                        </div>
                      </div>

                      {/* Progress Bar */}
                      <div className="w-full bg-[#0b0f19] h-2.5 rounded-full overflow-hidden mb-2 border border-slate-800">
                        <div
                          className={`h-full transition-all duration-300 rounded-full ${
                            task.status === "completed"
                              ? "bg-blue-500 shadow-sm shadow-blue-500/50"
                              : task.status === "error"
                              ? "bg-rose-500"
                              : "bg-emerald-500 shadow-sm shadow-emerald-500/50"
                          }`}
                          style={{ width: `${percent}%` }}
                        />
                      </div>

                      {/* Info & Percent */}
                      <div className="flex items-center justify-between text-xs text-slate-400 font-mono">
                        <span>
                          {formatBytes(task.transferred_bytes)} of {formatBytes(task.total_bytes)}
                        </span>
                        <span className="font-bold text-slate-300 font-sans">{percent}%</span>
                      </div>
                    </div>
                  );
                })
              )}
            </div>
          </div>
        )}
      </main>

      {/* ================= SETTINGS / GOOGLE OAUTH MODAL ================= */}
      {showSettings && (
        <div className="fixed inset-0 bg-black/80 backdrop-blur-md flex items-center justify-center p-5 z-50">
          <div className="bg-[#111827] border border-slate-800 w-full max-w-xl rounded-3xl p-7 shadow-2xl">
            <div className="flex items-center justify-between mb-4">
              <h3 className="text-lg font-bold flex items-center gap-2.5 text-white">
                <Settings className="w-5 h-5 text-blue-400" />
                Google Drive Connection Settings
              </h3>
              <button
                onClick={() => {
                  setShowSettings(false);
                  setPendingAuthUrl(null);
                }}
                className="text-slate-400 hover:text-slate-200 text-xs px-2 py-1 rounded-lg hover:bg-slate-800 transition"
              >
                ✕
              </button>
            </div>

            <p className="text-xs text-slate-400 mb-5 leading-relaxed">
              Enter your Google Cloud Console OAuth credentials below, then click Sign In to authenticate.
            </p>

            <div className="space-y-4 mb-6">
              <div>
                <label className="block text-xs font-semibold text-slate-300 mb-1.5 font-mono">
                  Google Client ID:
                </label>
                <input
                  type="text"
                  placeholder="...apps.googleusercontent.com"
                  value={clientId}
                  onChange={(e) => setClientId(e.target.value)}
                  className="w-full bg-[#0b0f19] border border-slate-800 rounded-xl px-3.5 py-2.5 text-xs text-slate-200 placeholder-slate-600 focus:outline-none focus:border-blue-500 font-mono transition"
                />
              </div>

              <div>
                <label className="block text-xs font-semibold text-slate-300 mb-1.5 font-mono">
                  Google Client Secret:
                </label>
                <input
                  type="password"
                  placeholder="GOCSPX-..."
                  value={clientSecret}
                  onChange={(e) => setClientSecret(e.target.value)}
                  className="w-full bg-[#0b0f19] border border-slate-800 rounded-xl px-3.5 py-2.5 text-xs text-slate-200 placeholder-slate-600 focus:outline-none focus:border-blue-500 font-mono transition"
                />
              </div>

              <button
                onClick={handleSaveCredentials}
                className="w-full py-2 bg-slate-900 hover:bg-slate-800 text-xs font-medium rounded-xl text-slate-300 border border-slate-800 transition"
              >
                Save Credentials
              </button>
            </div>

            {/* Login Section */}
            <div className="pt-4 border-t border-slate-800/80 space-y-3">
              <button
                onClick={handleStartLogin}
                disabled={authLoading}
                className="w-full py-3 bg-gradient-to-r from-blue-600 to-indigo-600 hover:from-blue-500 hover:to-indigo-500 text-white font-semibold rounded-xl text-sm shadow-lg shadow-blue-600/30 transition-all flex items-center justify-center gap-2 disabled:opacity-60"
              >
                <LogIn className="w-4 h-4" />
                {authLoading ? "Waiting for browser authorization..." : "Sign In with Google (OAuth 2.0)"}
              </button>

              {/* If waiting for browser approval, show direct link fallback */}
              {pendingAuthUrl && (
                <div className="bg-blue-950/40 border border-blue-800/50 rounded-2xl p-4 mt-3 space-y-2.5">
                  <p className="text-xs text-blue-300 leading-relaxed font-medium">
                    If the authorization page didn't open automatically, click below or copy the URL to your browser:
                  </p>
                  <div className="flex items-center gap-2">
                    <button
                      onClick={handleManualOpenBrowser}
                      className="flex-1 py-2 bg-blue-600 hover:bg-blue-500 text-white text-xs font-semibold rounded-xl transition flex items-center justify-center gap-1.5"
                    >
                      <ExternalLink className="w-3.5 h-3.5" />
                      Open in Browser
                    </button>
                    <button
                      onClick={handleCopyAuthUrl}
                      className="px-3 py-2 bg-slate-800 hover:bg-slate-700 text-slate-200 text-xs font-medium rounded-xl transition flex items-center gap-1.5 border border-slate-700"
                    >
                      {copied ? <Check className="w-3.5 h-3.5 text-emerald-400" /> : <Copy className="w-3.5 h-3.5" />}
                      {copied ? "Copied" : "Copy Link"}
                    </button>
                  </div>
                </div>
              )}
            </div>
          </div>
        </div>
      )}

      {/* ================= NEW FOLDER MODAL ================= */}
      {showNewFolderModal && (
        <div className="fixed inset-0 bg-black/80 backdrop-blur-md flex items-center justify-center p-5 z-50">
          <div className="bg-[#111827] border border-slate-800 w-full max-w-sm rounded-3xl p-6 shadow-2xl">
            <h3 className="text-base font-bold mb-3 flex items-center gap-2 text-white">
              <Folder className="w-4 h-4 text-amber-400" />
              Create New Folder
            </h3>
            <input
              type="text"
              placeholder="Folder name..."
              value={newFolderName}
              onChange={(e) => setNewFolderName(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handleCreateFolder()}
              autoFocus
              className="w-full bg-[#0b0f19] border border-slate-800 rounded-xl px-3.5 py-2.5 text-xs text-slate-200 placeholder-slate-600 focus:outline-none focus:border-blue-500 mb-5 transition"
            />
            <div className="flex items-center justify-end gap-2.5">
              <button
                onClick={() => setShowNewFolderModal(false)}
                className="text-xs text-slate-400 hover:text-slate-200 px-3 py-2 rounded-xl"
              >
                Cancel
              </button>
              <button
                onClick={handleCreateFolder}
                className="bg-blue-600 hover:bg-blue-500 text-white text-xs font-semibold px-4 py-2 rounded-xl shadow-md shadow-blue-600/25 transition"
              >
                Create
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
