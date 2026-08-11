import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  Activity,
  AlertCircle,
  AppWindow,
  ArrowRight,
  AudioLines,
  BadgeCheck,
  Box,
  Check,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  CircleGauge,
  Clock3,
  Cpu,
  Download,
  ExternalLink,
  FileImage,
  Film,
  Folder,
  FolderOpen,
  Gauge,
  HardDrive,
  Home,
  Image as ImageIcon,
  Info,
  Layers3,
  ListTodo,
  LoaderCircle,
  Maximize2,
  MemoryStick,
  Minus,
  Monitor,
  MonitorCog,
  MoreHorizontal,
  OctagonAlert,
  Package,
  Paperclip,
  Pause,
  Play,
  RefreshCw,
  Rocket,
  RotateCcw,
  Search,
  Settings,
  Settings2,
  ShieldCheck,
  SlidersHorizontal,
  Sparkles,
  Square,
  Trash2,
  Upload,
  Video,
  Volume2,
  VolumeX,
  WandSparkles,
  X,
  Zap,
  type LucideIcon,
} from "lucide-react";
import {
  cancelGeneration,
  cancelInstall,
  clearCache,
  chooseDirectory,
  chooseImages,
  chooseReferenceMedia,
  createInstallPlan,
  exportTextFile,
  getComfyStatus,
  getDefaultSettings,
  getInstallationStatus,
  generationVideoSource,
  isTauriRuntime,
  loadSettings,
  openExternal,
  openOutputFolder,
  pollGeneration,
  queueGeneration,
  saveSettings,
  scanEnvironment,
  scanHardware,
  startComfy,
  startInstall,
  stopComfy,
} from "./api";
import type {
  AppSettings,
  ComfyStatus,
  EnvironmentReport,
  GenerationJob,
  GenerationRequest,
  HardwareCheck,
  HardwareReport,
  InstallPlan,
  InstallProgress,
  InstallationStatus,
} from "./types";
import { DEFAULT_SETTINGS } from "./types";
import { basename, errorMessage, formatBytes, formatDate, mergeGenerationJob } from "./utils";
import brandLogo from "./assets/brand-logo-transparent.png";

type Page = "home" | "device" | "install" | "models" | "create" | "tasks" | "settings";

const LICENSE_VERSION = "2026-08-02";
const LICENSE_URL = "https://huggingface.co/MiniMaxAI/MiniMax-H3/blob/main/LICENSE";
const GENERATION_DRAFT_KEY = "magine-generation-draft-v1";
const GENERATION_RESOLUTIONS = [
  [608, 352], [864, 480], [960, 544], [1024, 576], [1344, 768], [1536, 864], [1920, 1088],
  [352, 608], [480, 864], [544, 960], [576, 1024], [768, 1344], [864, 1536], [1088, 1920],
  [1024, 1024],
] as const;
const normalizeWindowsPath = (value: string) => value.replaceAll("/", "\\");
const isGenerationModel = (path: string) => /(?:fl2va|ref2va)/i.test(basename(path));
const isReferenceModel = (path: string) => /ref2va/i.test(basename(path));
type GenerationMode = "text" | "image" | "frames" | "reference";
type ReferenceMediaKind = "image" | "video" | "audio";
type GenerationDraft = {
  prompt?: string;
  negative?: string;
  mode?: GenerationMode;
  selectedModel?: string;
  width?: number;
  height?: number;
  duration?: number;
  fps?: number;
  firstFrame?: string | null;
  lastFrame?: string | null;
  referencePaths?: string[];
};

const loadGenerationDraft = (): GenerationDraft => {
  try {
    const saved = window.localStorage.getItem(GENERATION_DRAFT_KEY);
    return saved ? JSON.parse(saved) as GenerationDraft : {};
  } catch {
    return {};
  }
};
const referenceMediaKind = (path: string): ReferenceMediaKind => {
  const name = basename(path).toLowerCase();
  if (/\.(mp4|mov|mkv|webm|avi)$/.test(name)) return "video";
  if (/\.(wav|mp3|flac|m4a|ogg|aac)$/.test(name)) return "audio";
  return "image";
};
const generationModelLabel = (path: string) => {
  const name = basename(path);
  const family = isReferenceModel(path) ? "Ref2VA · 全模态参考" : "FL2VA · 文本/首尾帧生视频";
  const precision = /fp8/i.test(name) ? "FP8" : /int8/i.test(name) ? "INT8" : "本地";
  return `MiniMax H3 ${family} · ${precision}`;
};

const navItems: { id: Page; label: string; icon: LucideIcon }[] = [
  { id: "home", label: "首页", icon: Home },
  { id: "device", label: "环境检测", icon: CheckCircle2 },
  { id: "install", label: "一键安装", icon: Download },
  { id: "models", label: "模型管理", icon: Box },
  { id: "create", label: "视频生成", icon: Video },
  { id: "tasks", label: "任务队列", icon: ListTodo },
  { id: "settings", label: "设置", icon: Settings },
];

const emptyComfyStatus: ComfyStatus = {
  state: "stopped",
  message: "尚未启动",
  pid: null,
  url: "http://127.0.0.1:8188",
};

function App() {
  const [page, setPage] = useState<Page>("home");
  const [report, setReport] = useState<HardwareReport | null>(null);
  const [settings, setSettings] = useState<AppSettings>(DEFAULT_SETTINGS);
  const [installation, setInstallation] = useState<InstallationStatus | null>(null);
  const [environment, setEnvironment] = useState<EnvironmentReport | null>(null);
  const [comfy, setComfy] = useState<ComfyStatus>(emptyComfyStatus);
  const [lastJob, setLastJob] = useState<GenerationJob | null>(null);
  const [recentJobs, setRecentJobs] = useState<GenerationJob[]>([]);
  const [loading, setLoading] = useState(true);
  const [globalError, setGlobalError] = useState<string | null>(null);
  const [notice, setNotice] = useState<{ message: string; tone: "success" | "info" | "warning" } | null>(null);
  const noticeTimer = useRef<number | null>(null);

  const notify = useCallback((message: string, tone: "success" | "info" | "warning" = "success") => {
    setNotice({ message, tone });
    if (noticeTimer.current) window.clearTimeout(noticeTimer.current);
    noticeTimer.current = window.setTimeout(() => setNotice(null), 3200);
  }, []);

  const refreshHardware = useCallback(async () => {
    setGlobalError(null);
    try {
      setReport(await scanHardware());
    } catch (error) {
      setGlobalError(`设备扫描失败：${errorMessage(error)}`);
    }
  }, []);

  const refreshInstallation = useCallback(async (nextSettings: AppSettings) => {
    try {
      setInstallation(await getInstallationStatus(nextSettings));
    } catch (error) {
      setInstallation({
        installed: false,
        healthy: false,
        comfyVersion: null,
        installedModels: [],
        runtimePath: null,
        message: errorMessage(error),
      });
    }
  }, []);

  const refreshEnvironment = useCallback(async (nextSettings: AppSettings) => {
    try {
      setEnvironment(await scanEnvironment(nextSettings));
    } catch (error) {
      setGlobalError(`环境扫描失败：${errorMessage(error)}`);
    }
  }, []);

  useEffect(() => {
    void (async () => {
      setLoading(true);
      try {
        const loaded = await loadSettings();
        const detected = await scanEnvironment(loaded);
        const aligned = {
          ...loaded,
          comfyPath: normalizeWindowsPath(detected.suggestedComfyPath ?? loaded.comfyPath),
          modelPath: normalizeWindowsPath(detected.suggestedModelPath ?? loaded.modelPath),
          outputPath: normalizeWindowsPath(loaded.outputPath),
        };
        const pathsAligned = aligned.comfyPath !== loaded.comfyPath || aligned.modelPath !== loaded.modelPath;
        if (pathsAligned) await saveSettings(aligned);
        setSettings(aligned);
        setEnvironment(pathsAligned ? await scanEnvironment(aligned) : detected);
        await Promise.all([
          refreshHardware(),
          refreshInstallation(aligned),
          getComfyStatus().then(setComfy),
        ]);
        if (pathsAligned) notify("已自动采用本机现有的 ComfyUI 与模型路径", "info");
      } finally {
        setLoading(false);
      }
    })();
  }, [notify, refreshHardware, refreshInstallation]);

  useEffect(() => {
    if (lastJob?.state !== "completed") return;
    setRecentJobs((current) => [lastJob, ...current.filter((job) => job.promptId !== lastJob.promptId)].slice(0, 20));
    notify("视频生成完成，已加载到预览与最近生成");
  }, [lastJob, notify]);

  useEffect(() => {
    if (!lastJob || !["queued", "running"].includes(lastJob.state) || !lastJob.clientId) return;
    const promptId = lastJob.promptId;
    const clientId = lastJob.clientId;
    let cancelled = false;
    let timer: number | null = null;
    let consecutiveFailures = 0;

    const tick = async () => {
      let keepPolling = true;
      try {
        const next = await pollGeneration(promptId, clientId, settings);
        if (cancelled) return;
        consecutiveFailures = 0;
        setLastJob((current) => current?.promptId === promptId
          ? mergeGenerationJob(current, next)
          : current);
        keepPolling = next.state === "queued" || next.state === "running";
      } catch (reason) {
        if (cancelled) return;
        consecutiveFailures += 1;
        if (consecutiveFailures >= 3) {
          keepPolling = false;
          setLastJob((current) => current?.promptId === promptId
            ? {
                ...current,
                state: "failed",
                message: `本地引擎连续三次无法返回任务状态：${errorMessage(reason)}`,
                estimatedRemainingSeconds: null,
              }
            : current);
        }
      }
      if (!cancelled && keepPolling) timer = window.setTimeout(tick, 2000);
    };

    timer = window.setTimeout(tick, 500);
    return () => {
      cancelled = true;
      if (timer !== null) window.clearTimeout(timer);
    };
  }, [lastJob?.clientId, lastJob?.promptId, lastJob?.state, settings]);

  const deleteRecentJob = useCallback((promptId: string) => {
    setRecentJobs((current) => current.filter((job) => job.promptId !== promptId));
    setLastJob((current) => current?.promptId === promptId ? null : current);
  }, []);

  const persistSettings = useCallback(async (next: AppSettings) => {
    await saveSettings(next);
    setSettings(next);
    await Promise.all([
      refreshHardware(),
      refreshInstallation(next),
      refreshEnvironment(next),
      getComfyStatus().then(setComfy),
    ]);
  }, [refreshEnvironment, refreshHardware, refreshInstallation]);

  return (
    <div className="app-shell">
      <a className="skip-link" href="#main-content">跳到主要内容</a>
      <WindowControls />
      <Sidebar page={page} onNavigate={setPage} report={report} comfy={comfy} />

      <main id="main-content" className="main-content" tabIndex={-1}>
        {globalError && (
          <div className="global-alert" role="alert">
            <AlertCircle size={18} />
            <span>{globalError}</span>
            <button className="icon-button" onClick={() => setGlobalError(null)} aria-label="关闭错误提示"><X size={17} /></button>
          </div>
        )}

        {loading || !report ? (
          <LoadingScreen />
        ) : (
          <div className="page-enter" key={page}>
            {page === "home" && (
              <HomePage
                report={report}
                installation={installation}
                comfy={comfy}
                settings={settings}
                lastJob={lastJob}
                onNavigate={setPage}
                onNotify={notify}
              />
            )}
            {page === "device" && (
              <DevicePage report={report} environment={environment} installation={installation} onRescan={async () => { await Promise.all([refreshHardware(), refreshInstallation(settings), refreshEnvironment(settings)]); }} onContinue={() => setPage("install")} onNotify={notify} />
            )}
            {page === "install" && (
              <InstallPage
                report={report}
                settings={settings}
                installation={installation}
                initialEnvironment={environment}
                onSettingsChange={persistSettings}
                onInstalled={(status) => {
                  setInstallation(status);
                  setPage("models");
                }}
                onNotify={notify}
              />
            )}
            {page === "models" && (
              <ModelsPage
                report={report}
                settings={settings}
                installation={installation}
                comfy={comfy}
                onComfyChange={setComfy}
                onSettingsChange={persistSettings}
                onInstallationChange={setInstallation}
                onNeedInstall={() => setPage("install")}
                onNotify={notify}
              />
            )}
            {page === "create" && (
              <CreatePage
                report={report}
                settings={settings}
                installation={installation}
                comfy={comfy}
                onComfyChange={setComfy}
                lastJob={lastJob}
                recentJobs={recentJobs}
                onJobChange={setLastJob}
                onDeleteRecentJob={deleteRecentJob}
                onNeedInstall={() => setPage("install")}
                onNotify={notify}
              />
            )}
            {page === "tasks" && (
              <TasksPage job={lastJob} settings={settings} onJobChange={setLastJob} onCreate={() => setPage("create")} onNotify={notify} />
            )}
            {page === "settings" && (
              <SettingsPage
                report={report}
                settings={settings}
                onSettingsChange={persistSettings}
                onRescan={async () => {
                  await Promise.all([
                    refreshHardware(),
                    refreshInstallation(settings),
                    refreshEnvironment(settings),
                    getComfyStatus().then(setComfy),
                  ]);
                }}
                onNotify={notify}
              />
            )}
          </div>
        )}
        {notice && <div className={`app-toast ${notice.tone}`} role="status"><CheckCircle2 size={17} /><span>{notice.message}</span><button className="icon-button" onClick={() => setNotice(null)} aria-label="关闭提示"><X size={15} /></button></div>}
      </main>
    </div>
  );
}

function WindowControls() {
  const act = async (kind: "minimize" | "maximize" | "close") => {
    if (!isTauriRuntime()) return;
    const window = getCurrentWindow();
    if (kind === "minimize") await window.minimize();
    if (kind === "maximize") await window.toggleMaximize();
    if (kind === "close") await window.close();
  };

  return (
    <div className="window-bar" data-tauri-drag-region>
      <div className="drag-area" data-tauri-drag-region />
      <button onClick={() => void act("minimize")} aria-label="最小化窗口"><Minus size={16} /></button>
      <button onClick={() => void act("maximize")} aria-label="最大化窗口"><Maximize2 size={14} /></button>
      <button className="close-window" onClick={() => void act("close")} aria-label="关闭窗口"><X size={17} /></button>
    </div>
  );
}

function Sidebar({ page, onNavigate, report, comfy }: { page: Page; onNavigate: (page: Page) => void; report: HardwareReport | null; comfy: ComfyStatus }) {
  const gpu = report?.gpus[0];
  return (
    <aside className="sidebar" aria-label="主导航">
      <div className="brand" data-tauri-drag-region>
        <img src={brandLogo} alt="MagineStudio 标志" className="brand-logo" />
        <div>
          <div className="brand-name">MagineStudio</div>
          <div className="brand-caption">MiniMax H3 · ComfyUI 本地部署</div>
        </div>
      </div>

      <nav className="nav-list">
        {navItems.map(({ id, label, icon: Icon }) => (
          <button
            className={`nav-item ${page === id ? "active" : ""}`}
            key={id}
            onClick={() => onNavigate(id)}
            aria-current={page === id ? "page" : undefined}
          >
            <Icon size={21} aria-hidden="true" />
            <span>{label}</span>
          </button>
        ))}
      </nav>

      <div className="sidebar-device glass-panel">
        <DeviceLine icon={Monitor} label="运行平台" value={window.navigator.platform || "Windows PC"} />
        <DeviceLine icon={AppWindow} label="操作系统" value={`${report?.osName ?? "Windows"} ${report?.osVersion ?? ""}`} />
        <DeviceLine icon={MemoryStick} label="运行内存" value={report ? formatBytes(report.totalMemoryBytes, 0) : "—"} />
        <DeviceLine icon={MonitorCog} label="显卡" value={gpu?.name ?? "检测中"} />
        <button className="sidebar-report" onClick={() => onNavigate("device")}>查看详细报告 <ChevronRight size={15} /></button>
      </div>

      <div className="sidebar-footer">
        <span>v1.0.0</span>
        <span className={`runtime-state runtime-${comfy.state}`}><i />本地服务：{comfy.state === "ready" ? "运行中" : "未启动"}</span>
      </div>
    </aside>
  );
}

function DeviceLine({ icon: Icon, label, value }: { icon: LucideIcon; label: string; value: string }) {
  return <div className="device-line"><Icon size={15} /><span>{label}</span><strong title={value}>{value}</strong></div>;
}

function LoadingScreen() {
  return (
    <div className="loading-screen" role="status" aria-live="polite">
      <div className="scan-orbit"><CircleGauge size={38} /></div>
      <h1>正在扫描本机运行环境</h1>
      <p>检查显卡、显存、内存、磁盘与 ComfyUI 状态，数据不会离开这台电脑。</p>
    </div>
  );
}

function PageHeading({ icon: Icon, title, suffix, description, action }: { icon?: LucideIcon; title: string; suffix?: string; description: string; action?: React.ReactNode }) {
  return (
    <header className="page-heading">
      <div className="page-heading-main">
        {Icon && <div className="heading-icon"><Icon size={29} /></div>}
        <div><h1>{title}{suffix && <span>{suffix}</span>}</h1><p>{description}</p></div>
      </div>
      {action}
    </header>
  );
}

function HomePage({ report, installation, comfy, settings, lastJob, onNavigate, onNotify }: {
  report: HardwareReport;
  installation: InstallationStatus | null;
  comfy: ComfyStatus;
  settings: AppSettings;
  lastJob: GenerationJob | null;
  onNavigate: (page: Page) => void;
  onNotify: (message: string, tone?: "success" | "info" | "warning") => void;
}) {
  const gpu = report.gpus[0];
  const disk = diskForPath(report.disks, settings.modelPath);
  const ready = Boolean(installation?.installed && installation.healthy);
  const recent = lastJob ? [lastJob] : [];

  return (
    <section className="page page-home">
      <div className="home-layout">
        <div className="home-primary">
          <section className="hero-card glass-panel">
            <div className="hero-copy">
              <div className={`hero-status ${ready ? "success" : "warning"}`}><span><Check size={22} /></span><div><h2>{ready ? "本地环境已就绪" : "本地环境等待部署"}</h2><p>{ready ? "核心组件已经安装，开始生成时会自动启动本地引擎" : "完成环境检测与一键安装后即可本地生成视频"}</p></div></div>
              <div className="current-model">
                <span>当前模型</span>
                <strong>MiniMax H3 <em>{ready ? "已安装" : "待安装"}</em></strong>
                <small>版本：v1.0.0　类型：视频生成模型　参数规模：80B</small>
              </div>
            </div>
            <div className="h3-emblem"><div>H3</div></div>
            <button className="button primary hero-cta" onClick={() => onNavigate("create")}><Play size={19} fill="currentColor" />开始生成</button>
            <div className="hero-actions">
              <button onClick={() => comfy.state === "ready" ? openExternal(comfy.url) : onNavigate(installation?.installed ? "models" : "install")}><ExternalLink size={16} />打开 ComfyUI</button>
              <button onClick={() => openOutputFolder(settings.outputPath)}><FolderOpen size={16} />输出目录</button>
              <button onClick={() => onNavigate("models")}><RefreshCw size={16} />模型切换</button>
              <button onClick={() => onNavigate("settings")}><Settings2 size={16} />设置</button>
            </div>
          </section>

          <div className="home-stats">
            <HomeStat icon={MonitorCog} title="GPU / 显存" value={gpu?.vramMb ? `${(gpu.vramMb / 1024).toFixed(1)} GB` : "未识别"} sub={gpu?.name ?? "暂无显卡信息"} percent={gpu?.vramMb ? 32 : 0} tone="violet" />
            <HomeStat icon={HardDrive} title="存储空间" value={disk ? formatBytes(disk.availableBytes, 0) : "—"} sub={disk ? `${disk.name} · 可用空间` : "暂无磁盘信息"} percent={disk ? Math.round((1 - disk.availableBytes / disk.totalBytes) * 100) : 0} tone="cyan" />
            <HomeStat icon={Box} title="当前模型" value="MiniMax H3" sub={ready ? "已安装" : "尚未部署"} tone="blue" />
            <HomeStat icon={Layers3} title="当前会话任务" value={lastJob ? "1" : "0"} sub={lastJob ? "已有真实任务记录" : "等待首个任务"} tone="purple" />
          </div>

          <section className="quick-panel glass-panel">
            <h3>快速开始</h3>
            <div className="quick-grid">
              <QuickCard icon={ShieldCheck} title="环境检测" sub="检查硬件和软件环境" onClick={() => onNavigate("device")} tone="cyan" />
              <QuickCard icon={Download} title="一键安装" sub="自动安装所需依赖" onClick={() => onNavigate("install")} tone="blue" />
              <QuickCard icon={Box} title="模型管理" sub="下载、切换与管理模型" onClick={() => onNavigate("models")} tone="purple" />
              <QuickCard icon={Video} title="视频生成" sub="开始创作你的作品" onClick={() => onNavigate("create")} tone="orange" />
            </div>
          </section>

          <section className="recent-panel glass-panel">
            <div className="section-title"><h3>最近任务</h3><button onClick={() => onNavigate("tasks")}>查看全部 <ChevronRight size={15} /></button></div>
            <div className="recent-list">
              {recent.map((job, index) => (
                <div className="recent-row" key={`${job.promptId}-${index}`}>
                  <TaskThumb index={index} />
                  <div className="recent-name"><strong>{job.filename || "MiniMax H3 本地生成任务"}</strong><span>{job.message}</span></div>
                  <span className={`task-state ${job.state}`}>{job.state === "running" ? "生成中" : job.state === "failed" ? "失败" : job.state === "queued" ? "排队中" : "已完成"}</span>
                  <div className="recent-time"><strong>{job.state === "completed" ? "已完成" : "处理中"}</strong><span>当前会话</span></div>
                  <button className="icon-button" aria-label="打开任务目录" onClick={() => openOutputFolder(settings.outputPath)}><FolderOpen size={17} /></button>
                  <button className="round-play" aria-label="查看任务" onClick={() => onNavigate("tasks")}><Play size={14} /></button>
                </div>
              ))}
              {!recent.length && <PanelEmpty icon={Clock3} title="暂无真实任务" description="生成第一个视频后，任务会显示在这里。" action={<button className="empty-action" onClick={() => onNavigate("create")}>开始生成 <ArrowRight size={14} /></button>} />}
            </div>
          </section>
        </div>

        <aside className="home-aside">
          <section className="health-panel glass-panel">
            <h3>系统健康</h3>
            <HealthRow icon={Cpu} label="NVIDIA CUDA" ok={Boolean(gpu?.cudaAvailable)} okLabel="可用" pendingLabel="未检测" />
            <HealthRow icon={Sparkles} label="独立 Python 环境" ok={Boolean(installation?.runtimePath)} okLabel="已检测" pendingLabel="未配置" />
            <HealthRow icon={Box} label="ComfyUI" ok={comfy.state === "ready"} okLabel="运行中" pendingLabel={installation?.installed ? "未启动" : "未安装"} />
            <HealthRow icon={Package} label="模型库" ok={Boolean(installation?.installedModels.length)} okLabel="已检测" pendingLabel="未安装" />
            <button className="wide-ghost" onClick={() => onNavigate("device")}>查看健康详情 <ChevronRight size={15} /></button>
          </section>

          <section className="workflow-panel glass-panel">
            <div className="section-title"><h3>本地工作流</h3><button onClick={() => onNavigate("models")}>管理工作流 <ChevronRight size={15} /></button></div>
            <PanelEmpty icon={Layers3} title={installation?.installed ? "等待读取本地工作流" : "尚未安装工作流"} description={installation?.installed ? "模型管理页会显示实际安装的工作流文件。" : "完成一键安装后，这里只展示真实的本地工作流。"} action={<button className="empty-action" onClick={() => onNavigate(installation?.installed ? "models" : "install")}>{installation?.installed ? "查看模型管理" : "前往安装"}</button>} compact />
          </section>

          <section className="notice-panel glass-panel">
            <div className="section-title"><h3>客户端状态</h3><span className="version-badge">v1.0.0</span></div>
            <div className="client-state-list"><p><span>最近硬件扫描</span><strong>{formatDate(report.scannedAt)}</strong></p><p><span>本地安装状态</span><strong>{installation?.installed ? "已安装" : "未安装"}</strong></p><p><span>ComfyUI 服务</span><strong>{comfy.state === "ready" ? "运行中" : "未启动"}</strong></p></div>
            <button className="notice-more" onClick={() => { onNavigate("settings"); onNotify("已打开设置中心", "info"); }}>检查客户端设置 <ChevronRight size={15} /></button>
          </section>
        </aside>
      </div>
    </section>
  );
}

function HomeStat({ icon: Icon, title, value, sub, percent, tone }: { icon: LucideIcon; title: string; value: string; sub: string; percent?: number; tone: string }) {
  return (
    <article className="home-stat glass-panel">
      <div className={`stat-icon ${tone}`}><Icon size={20} /></div>
      <div className="stat-copy"><span>{title}</span><small>{sub}</small></div>
      <strong>{value}</strong>
      {percent !== undefined && <div className="mini-meter"><i style={{ width: `${Math.min(100, percent)}%` }} /></div>}
    </article>
  );
}

function QuickCard({ icon: Icon, title, sub, onClick, tone }: { icon: LucideIcon; title: string; sub: string; onClick: () => void; tone: string }) {
  return <button className={`quick-card ${tone}`} onClick={onClick}><div><strong>{title}</strong><span>{sub}</span></div><Icon size={38} /></button>;
}

function HealthRow({ icon: Icon, label, ok, okLabel = "运行正常", pendingLabel = "待配置" }: { icon: LucideIcon; label: string; ok: boolean; okLabel?: string; pendingLabel?: string }) {
  return <div className="health-row"><Icon size={17} /><span>{label}</span><strong className={ok ? "ok" : "pending"}><CheckCircle2 size={15} />{ok ? okLabel : pendingLabel}</strong></div>;
}

function DevicePage({ report, environment, installation, onRescan, onContinue, onNotify }: { report: HardwareReport; environment: EnvironmentReport | null; installation: InstallationStatus | null; onRescan: () => Promise<void>; onContinue: () => void; onNotify: (message: string, tone?: "success" | "info" | "warning") => void }) {
  const [rescanning, setRescanning] = useState(false);
  const gpu = report.gpus[0];
  const totalDiskBytes = report.disks.reduce((total, disk) => total + disk.totalBytes, 0);
  const availableDiskBytes = report.disks.reduce((total, disk) => total + disk.availableBytes, 0);
  const score = report.tier === "comfortable" ? 94 : report.tier === "usable" ? 82 : report.tier === "experimental" ? 68 : 42;
  const runScan = async () => { setRescanning(true); try { await onRescan(); onNotify("硬件、软件、环境和模型扫描已刷新"); } finally { setRescanning(false); } };
  const exportReport = async () => {
    const target = await exportTextFile(`MagineStudio-环境检测-${new Date().toISOString().slice(0, 10)}.json`, JSON.stringify({ hardware: report, environment, installation }, null, 2));
    if (target) onNotify(`检测报告已导出：${target}`);
  };
  const component = (id: string) => environment?.components.find((item) => item.id === id);
  const softwareRows = [
    { id: "python", icon: Sparkles },
    { id: "git", icon: Package },
    { id: "cuda", icon: Zap },
    { id: "comfyui", icon: Box },
    { id: "models", icon: Layers3 },
    { id: "configuration", icon: Settings2 },
  ].map(({ id, icon }) => {
    const item = component(id);
    return {
      icon,
      title: item?.name ?? id,
      value: item?.version ?? (item?.state === "ready" ? "已检测" : item?.state === "partial" ? "部分就绪" : "未检测到"),
      detail: item?.path ?? item?.message ?? "正在读取本机状态",
      status: item?.state === "ready" ? "正常" : item?.state === "partial" ? "待配置" : "待安装",
    };
  });
  const detailRows = [
    { icon: Cpu, title: "CPU", value: report.cpuName, detail: `${report.physicalCores ?? "—"} 核心 / ${report.logicalCores} 线程`, status: "优秀" },
    { icon: MonitorCog, title: "GPU", value: gpu?.name ?? "未识别", detail: gpu?.vramMb ? `${(gpu.vramMb / 1024).toFixed(0)} GB 显存` : "显存未知", status: gpu?.cudaAvailable ? "优秀" : "检查" },
    { icon: MemoryStick, title: "显存 (VRAM)", value: gpu?.vramMb ? `${(gpu.vramMb / 1024).toFixed(0)} GB` : "未知", detail: "16 GB 可运行，24 GB 及以上更舒适", status: gpu?.vramMb && gpu.vramMb >= 16384 ? "正常" : "注意" },
    { icon: MemoryStick, title: "内存 (RAM)", value: formatBytes(report.totalMemoryBytes, 0), detail: `可用 ${formatBytes(report.availableMemoryBytes, 0)}`, status: report.totalMemoryBytes >= 32 * 1024 ** 3 ? "优秀" : "注意" },
    { icon: HardDrive, title: "本地磁盘", value: report.disks.length ? `${report.disks.length} 个 · ${formatBytes(totalDiskBytes, 0)}` : "未知", detail: report.disks.length ? `合计可用 ${formatBytes(availableDiskBytes, 0)}；路径页按盘符分别显示` : "无法读取", status: report.disks.length ? "正常" : "检查" },
    { icon: Activity, title: "网络状态", value: navigator.onLine ? "已连接" : "已断开", detail: "用于下载安装组件", status: navigator.onLine ? "正常" : "检查" },
    { icon: AppWindow, title: "操作系统", value: `${report.osName} ${report.osVersion}`, detail: `${report.architecture} · 支持本地部署`, status: "正常" },
    ...softwareRows,
  ];

  return (
    <section className="page page-device">
      <PageHeading icon={ShieldCheck} title="环境检测" suffix=" / 硬件与运行环境" description="全面检测您的硬件配置与软件环境，评估运行 MiniMax H3 的兼容性" />
      <div className="result-banner glass-panel">
        <div className="result-icon"><Check size={28} /></div>
        <div><h2>检测结果：{report.tierLabel} MiniMax H3</h2><p>{report.summary}，推荐按建议预设开始创作。</p></div>
        <div className="scan-meta"><span><Clock3 size={15} />检测时间：{formatDate(report.scannedAt)}</span><span><Info size={15} />检测版本：v1.0.0</span></div>
        <button className="button secondary" onClick={runScan} disabled={rescanning}><RefreshCw className={rescanning ? "spin" : ""} size={17} />重新扫描</button>
      </div>

      <div className="device-layout">
        <section className="detail-panel glass-panel">
          <h3>详细检测结果</h3>
          <div className="detail-grid">
            {detailRows.map((item) => <EnvironmentCard key={item.title} {...item} />)}
          </div>
        </section>
        <aside className="device-aside">
          <section className="score-panel glass-panel">
            <div className="section-title"><h3>综合评分</h3><span>硬件性能</span></div>
            <RadarScore score={score} />
            <p>能力分档：<strong>{report.tierLabel}</strong>　依据本机显卡、显存、内存与系统检测</p>
          </section>
          <section className="compat-panel glass-panel">
            <div className="section-title"><h3>模型适配建议</h3><Info size={16} /></div>
            <div className="compat-head"><span>模型</span><span>最低显存要求</span><span>当前适配</span><span>建议</span></div>
            {Array.from(new Set((installation?.installedModels ?? []).filter(isGenerationModel).map((path) => isReferenceModel(path) ? "MiniMax H3 Ref2VA" : "MiniMax H3 FL2VA"))).map((name) => {
              const suitable = Boolean(gpu?.cudaAvailable && gpu?.vramMb && gpu.vramMb >= 16 * 1024);
              return <div className="compat-row" key={name}><span>{name}</span><span>建议 ≥ 16 GB</span><span><CheckCircle2 size={15} />{gpu?.vramMb ? `${Math.round(gpu.vramMb / 1024)} GB` : "未知"}</span><strong>{suitable ? report.tierLabel : "低于建议配置"}</strong></div>;
            })}
            {!installation?.installedModels.some(isGenerationModel) && <div className="compat-row"><span>MiniMax H3</span><span>建议 ≥ 16 GB</span><span>尚未安装</span><strong>待部署</strong></div>}
          </section>
          <section className="risk-panel glass-panel">
            <h3><AlertCircle size={18} />待处理项 / 风险提示</h3>
            <div><span className="orange-dot" /><p><strong>{gpu?.driverVersion ? "显卡驱动信息" : "未读取到显卡驱动"}</strong><small>当前版本 {gpu?.driverVersion ?? "未知"}；是否有新版以 NVIDIA 官方页面为准</small></p><button onClick={() => openExternal("https://www.nvidia.com/Download/index.aspx?lang=cn")}>查看官方驱动</button></div>
          </section>
        </aside>
      </div>
      <div className="bottom-actions">
        <button className="button secondary" onClick={runScan}><RefreshCw size={17} />重新扫描</button>
        <button className="button secondary" onClick={exportReport}><Download size={17} />导出检测报告</button>
        <button className="button primary" onClick={onContinue}><Settings2 size={17} />修复环境问题</button>
      </div>
    </section>
  );
}

function EnvironmentCard({ icon: Icon, title, value, detail, status }: { icon: LucideIcon; title: string; value: string; detail: string; status: string }) {
  const good = status === "优秀" || status === "正常";
  return (
    <article className="environment-card">
      <div className="env-icon"><Icon size={21} /></div>
      <div><span>{title}</span><strong title={value}>{value}</strong><small>{detail}</small></div>
      <div className="bench"><span>检测结果</span><strong>{good ? "已通过" : status}</strong><i><b style={{ width: good ? "100%" : "55%" }} /></i></div>
      <em className={good ? "good" : "warn"}>{status}</em>
    </article>
  );
}

function RadarScore({ score }: { score: number }) {
  const points = "90,12 156,58 132,136 48,136 24,58";
  const inner = "90,26 142,63 122,122 58,122 38,63";
  return (
    <div className="radar-score" aria-label={`综合评分 ${score}`}>
      <svg viewBox="0 0 180 155" role="img"><polygon points={points} className="radar-outer" /><polygon points={inner} className="radar-inner" /><polyline points={`${points} 90,12`} className="radar-lines" /></svg>
      <div><strong>{score}</strong><span>总分</span></div>
    </div>
  );
}

function InstallPage({ report, settings, installation, initialEnvironment, onSettingsChange, onInstalled, onNotify }: {
  report: HardwareReport;
  settings: AppSettings;
  installation: InstallationStatus | null;
  initialEnvironment: EnvironmentReport | null;
  onSettingsChange: (settings: AppSettings) => Promise<void>;
  onInstalled: (status: InstallationStatus) => void;
  onNotify: (message: string, tone?: "success" | "info" | "warning") => void;
}) {
  const [draft, setDraft] = useState(settings);
  const [includeReference, setIncludeReference] = useState(false);
  const [plan, setPlan] = useState<InstallPlan | null>(null);
  const [progress, setProgress] = useState<InstallProgress | null>(null);
  const [installing, setInstalling] = useState(false);
  const [planning, setPlanning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [environment, setEnvironment] = useState<EnvironmentReport | null>(initialEnvironment);
  const [showLog, setShowLog] = useState(true);

  useEffect(() => setDraft(settings), [settings]);
  const request = useMemo(() => ({
    comfyPath: draft.comfyPath,
    modelPath: draft.modelPath,
    outputPath: draft.outputPath,
    includeReferenceModel: includeReference,
    licenseAccepted: draft.licenseAccepted,
    regionConfirmed: draft.regionConfirmed,
    licenseVersion: LICENSE_VERSION,
  }), [draft, includeReference]);

  useEffect(() => {
    let cancelled = false;
    const timer = window.setTimeout(() => {
      setPlanning(true);
      setError(null);
      setPlan(null);
      Promise.allSettled([createInstallPlan(request), scanEnvironment(draft, includeReference)])
        .then(([planResult, environmentResult]) => {
          if (cancelled) return;
          if (planResult.status === "fulfilled") setPlan(planResult.value);
          else setError(errorMessage(planResult.reason));
          if (environmentResult.status === "fulfilled") setEnvironment(environmentResult.value);
          else setError((current) => current ?? errorMessage(environmentResult.reason));
        })
        .finally(() => { if (!cancelled) setPlanning(false); });
    }, 220);
    return () => { cancelled = true; window.clearTimeout(timer); };
  }, [draft, includeReference, request]);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    let unlisten: (() => void) | undefined;
    void listen<InstallProgress>("install-progress", (event) => setProgress(event.payload)).then((fn) => { unlisten = fn; });
    return () => unlisten?.();
  }, []);

  const updateDraft = (patch: Partial<AppSettings>) => setDraft((current) => ({ ...current, ...patch }));
  const pickPath = async (key: "comfyPath" | "modelPath" | "outputPath") => {
    const selected = await chooseDirectory(draft[key]);
    if (selected) {
      const next = { ...draft, [key]: normalizeWindowsPath(selected) };
      setDraft(next);
      await onSettingsChange(next);
      onNotify(`${key === "outputPath" ? "输出目录" : key === "modelPath" ? "模型目录" : "ComfyUI 目录"}已保存`, "info");
    }
  };
  const saveEditedPaths = async () => {
    if (draft.comfyPath === settings.comfyPath && draft.modelPath === settings.modelPath && draft.outputPath === settings.outputPath) return;
    await onSettingsChange({
      ...draft,
      comfyPath: normalizeWindowsPath(draft.comfyPath),
      modelPath: normalizeWindowsPath(draft.modelPath),
      outputPath: normalizeWindowsPath(draft.outputPath),
    });
  };
  const beginInstall = async () => {
    setError(null);
    setInstalling(true);
    setProgress({ stage: "preflight", stageLabel: "准备安装", fileName: null, fileDownloadedBytes: 0, fileTotalBytes: 0, totalDownloadedBytes: 0, totalBytes: plan?.downloadBytes ?? 0, percent: 1, message: "正在检查目录和可用空间…", canCancel: true });
    const accepted = { ...draft, acceptedLicenseVersion: LICENSE_VERSION, acceptedAt: new Date().toISOString() };
    try {
      await onSettingsChange(accepted);
      const status = await startInstall({ ...request, licenseAccepted: accepted.licenseAccepted, regionConfirmed: accepted.regionConfirmed });
      setEnvironment(await scanEnvironment(accepted, includeReference));
      setProgress((current) => current ? { ...current, stage: "complete", stageLabel: "安装完成", percent: 100, message: "本地生成环境已准备好。", canCancel: false } : current);
      onNotify("本地生成环境部署完成");
      onInstalled(status);
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setInstalling(false);
    }
  };
  const stopInstall = async () => { await cancelInstall(); onNotify("已请求取消安装，当前文件会安全结束后停止，已下载内容可用于续传", "warning"); };
  const adoptDetectedPaths = () => {
    if (!environment) return;
    updateDraft({
      comfyPath: environment.suggestedComfyPath ?? draft.comfyPath,
      modelPath: environment.suggestedModelPath ?? draft.modelPath,
    });
    onNotify("已采用本机检测到的真实安装路径", "info");
  };
  const spacesReady = environment?.pathSpaces.every((space) => space.sufficient) ?? false;
  const canInstall = Boolean(draft.comfyPath.trim() && draft.modelPath.trim() && draft.outputPath.trim() && draft.licenseAccepted && draft.regionConfirmed && report.tier !== "unsupported" && plan && spacesReady && !error && !planning && !installing);
  const pct = Math.round(progress?.percent ?? (installation?.healthy ? 100 : 0));
  const detectedComponent = (id: string) => environment?.components.find((component) => component.id === id);
  const taskRows = [
    { id: "python", name: "Python 运行环境", icon: Sparkles, start: 0, end: 12 },
    { id: "git", name: "Git（可选）", icon: Package, start: 12, end: 20 },
    { id: "cuda", name: "NVIDIA 驱动 / CUDA", icon: Zap, start: 20, end: 35 },
    { id: "comfyui", name: "ComfyUI", icon: Box, start: 35, end: 58 },
    { id: "models", name: "MiniMax H3 模型", icon: BadgeCheck, start: 58, end: 94 },
    { id: "configuration", name: "模型路径配置", icon: Layers3, start: 94, end: 100 },
  ];

  return (
    <section className="page page-install">
      <Stepper steps={["扫描硬件", "选择路径", "安装依赖", "下载模型", "写入配置", "完成部署"]} active={installing ? Math.max(1, Math.ceil(pct / 20)) : installation?.healthy ? 5 : 0} />
      <div className="install-top">
        <section className="install-options glass-panel">
          <h3>安装选项与路径设置</h3>
          {environment?.alignmentMessage && (
            <div className="path-alignment-notice" role="status">
              <CheckCircle2 size={16} />
              <span><strong>发现本机现有配置</strong><small>{environment.alignmentMessage}</small></span>
              <button onClick={adoptDetectedPaths}>采用检测路径</button>
            </div>
          )}
          <div className="install-paths">
            <CompactPath label="ComfyUI 安装位置" value={draft.comfyPath} onChange={(value) => updateDraft({ comfyPath: value })} onBlur={saveEditedPaths} onPick={() => pickPath("comfyPath")} />
            <CompactPath label="模型下载位置" value={draft.modelPath} onChange={(value) => updateDraft({ modelPath: value })} onBlur={saveEditedPaths} onPick={() => pickPath("modelPath")} />
            <CompactPath label="输出目录" value={draft.outputPath} onChange={(value) => updateDraft({ outputPath: value })} onBlur={saveEditedPaths} onPick={() => pickPath("outputPath")} />
          </div>
          <ToggleRow label="下载多参考扩展模型" checked={includeReference} onChange={setIncludeReference} />
          <label className="consent-row"><input type="checkbox" checked={draft.licenseAccepted} onChange={(event) => updateDraft({ licenseAccepted: event.target.checked })} /><span><Check size={13} /></span>我已阅读并同意 <button onClick={() => openExternal(LICENSE_URL)}>MiniMax H3 社区许可</button></label>
          <label className="consent-row"><input type="checkbox" checked={draft.regionConfirmed} onChange={(event) => updateDraft({ regionConfirmed: event.target.checked })} /><span><Check size={13} /></span>我确认当前所在地区符合模型许可要求</label>
          <div className="space-summary"><span>本次仍需下载：<strong>{plan ? formatBytes(plan.downloadBytes, 1) : "计算中"}</strong></span><span>按所选路径实时扫描</span></div>
          <div className="path-space-list" aria-live="polite">
            {(environment?.pathSpaces ?? []).map((space) => <div className={space.sufficient ? "space-ok" : "space-low"} key={space.id}><span><HardDrive size={14} />{space.label} · {space.mountPoint ?? "未知盘符"}</span><strong>{space.availableBytes === null ? "无法读取" : `可用 ${formatBytes(space.availableBytes, 1)}`}</strong><small>本项需要 {formatBytes(space.requiredBytes, 1)}</small></div>)}
            {!environment && <div className="space-scanning"><LoaderCircle className="spin" size={15} />正在读取三个路径所在磁盘…</div>}
          </div>
        </section>

        <section className="install-tasks glass-panel">
          <h3>安装任务清单 <small>{environment?.summary ?? "正在检测本机已有配置…"}</small></h3>
          <div className="task-table-head"><span>组件</span><span>状态</span><span>进度</span><span>速度</span><span>剩余时间</span></div>
          {taskRows.map(({ id, name, icon: Icon, start, end }) => {
            const detected = detectedComponent(id);
            const installPct = pct >= end ? 100 : pct <= start ? 0 : Math.round(((pct - start) / (end - start)) * 100);
            const rowPct = installing ? installPct : detected?.state === "ready" ? 100 : detected?.state === "partial" ? 50 : 0;
            const state = installing && installPct > 0 && installPct < 100 ? "正在处理" : detected?.state === "ready" ? "已检测" : detected?.state === "partial" ? "需补全" : id === "git" ? "可选缺失" : "待安装";
            const stateClass = detected?.state === "ready" || (installing && installPct === 100) ? "done" : installing && installPct > 0 ? "working" : "waiting";
            return <div className="install-task-row" key={id} title={detected?.message}><span><Icon size={20} />{name}</span><strong className={stateClass}>{stateClass === "done" ? <CheckCircle2 size={14} /> : stateClass === "working" ? <LoaderCircle className="spin" size={14} /> : <Clock3 size={14} />}{state}</strong><span className="row-progress"><i><b style={{ width: `${rowPct}%` }} /></i>{rowPct}%</span><span>{installing && installPct > 0 && installPct < 100 ? "进行中" : "-"}</span><span>{detected?.version ?? "--"}</span></div>;
          })}
          <div className="install-summary-bar">
            <span>总体进度 <strong>{Math.min(6, Math.ceil(pct / 17))}/6</strong><i><b style={{ width: `${pct}%` }} /></i><em>{pct}%</em></span>
            <span><Activity size={20} />当前文件<strong>{progress?.fileName ? basename(progress.fileName) : "—"}</strong></span>
            <span><Download size={21} />已下载<strong>{progress ? `${formatBytes(progress.totalDownloadedBytes)} / ${formatBytes(progress.totalBytes)}` : "0 B"}</strong></span>
            <span><Clock3 size={22} />安装状态<strong>{installing ? progress?.stageLabel ?? "进行中" : installation?.healthy ? "已完成" : installation?.installed ? "需要修复" : "未开始"}</strong></span>
          </div>
        </section>
      </div>

      <section className="deploy-log glass-panel">
        <div className="section-title"><h3>部署日志（实时）</h3><button onClick={() => { setShowLog(false); onNotify("当前部署日志已清空", "info"); }}><Trash2 size={15} />清空日志</button></div>
        <div className="terminal-log">
          {showLog ? <><p><time>[环境]</time> <b>[信息]</b> 当前硬件分档：{report.tierLabel}</p><p><time>[路径]</time> <b>[信息]</b> ComfyUI：{draft.comfyPath}</p><p><time>[方案]</time> <strong>[就绪]</strong> {plan ? `预计下载 ${formatBytes(plan.downloadBytes)}` : "正在生成安装方案"}</p>{progress && <p><time>[当前]</time> <b>[信息]</b> {progress.message}{progress.fileName ? ` · ${progress.fileName}` : ""}</p>}{error && <p className="log-error"><time>[错误]</time> {error}</p>}</> : <p><time>[信息]</time> 日志已清空，新的安装事件会继续显示。</p>}
        </div>
      </section>

      <div className="install-actionbar">
        <button className="button secondary" onClick={() => window.location.reload()}><RefreshCw size={18} />重新检测</button>
        <button className="button primary wide" onClick={beginInstall} disabled={!canInstall}><Rocket size={19} />{installation?.installed ? "检查并修复部署" : "开始一键部署"}</button>
        <button className="button danger" onClick={stopInstall} disabled={!installing || progress?.canCancel === false}><X size={18} />取消</button>
      </div>
    </section>
  );
}

function Stepper({ steps, active }: { steps: string[]; active: number }) {
  return <div className="stepper glass-panel">{steps.map((step, index) => <div className={`${index <= active ? "active" : ""} ${index < active ? "complete" : ""}`} key={step}><span>{index < active ? <Check size={16} /> : index + 1}</span><strong>{step}</strong>{index < steps.length - 1 && <i />}</div>)}</div>;
}

function CompactPath({ label, value, onChange, onPick, onBlur, readOnly = false }: { label: string; value: string; onChange: (value: string) => void; onPick: () => void; onBlur?: () => void; readOnly?: boolean }) {
  return <label className="compact-path"><span><Folder size={17} />{label}</span><div><input value={value} onChange={(event) => onChange(event.target.value)} onBlur={onBlur} readOnly={readOnly} /><button type="button" onClick={onPick}>浏览</button></div></label>;
}

function ToggleRow({ label, checked, onChange }: { label: string; checked: boolean; onChange: (checked: boolean) => void }) {
  return <label className="toggle-row"><span>{label}</span><input type="checkbox" checked={checked} onChange={(event) => onChange(event.target.checked)} /><i><b /></i><em>{checked ? "是" : "否"}</em></label>;
}

function ModelsPage({ report, settings, installation, comfy, onComfyChange, onSettingsChange, onInstallationChange, onNeedInstall, onNotify }: {
  report: HardwareReport;
  settings: AppSettings;
  installation: InstallationStatus | null;
  comfy: ComfyStatus;
  onComfyChange: (status: ComfyStatus) => void;
  onSettingsChange: (settings: AppSettings) => Promise<void>;
  onInstallationChange: (status: InstallationStatus) => void;
  onNeedInstall: () => void;
  onNotify: (message: string, tone?: "success" | "info" | "warning") => void;
}) {
  const [draft, setDraft] = useState(settings);
  const [busy, setBusy] = useState(false);
  const [query, setQuery] = useState("");
  const [showLogs, setShowLogs] = useState(true);
  useEffect(() => setDraft(settings), [settings]);
  const disk = diskForPath(report.disks, draft.modelPath);
  const gpu = report.gpus[0];
  const toggleComfy = async () => {
    setBusy(true);
    try {
      const status = comfy.state === "ready" ? await stopComfy() : await startComfy(settings);
      onComfyChange(status);
      onNotify(comfy.state === "ready" ? "ComfyUI 已停止" : "ComfyUI 已启动");
    } catch (reason) { onNotify(errorMessage(reason), "warning"); } finally { setBusy(false); }
  };
  const pick = async (key: keyof Pick<AppSettings, "modelPath" | "outputPath">) => { const value = await chooseDirectory(draft[key]); if (value) setDraft({ ...draft, [key]: value }); };
  const allInstalledModels = installation?.installedModels ?? [];
  const enabledModels = settings.enabledModels ?? allInstalledModels;
  const enabledModelSet = useMemo(() => new Set(enabledModels), [enabledModels]);
  const installedModels = allInstalledModels.filter((model) => basename(model).toLowerCase().includes(query.trim().toLowerCase()));
  const runtimeLogs = [
    `ComfyUI 服务状态：${comfy.state}，地址：${comfy.url}`,
    `GPU：${gpu?.name ?? "未识别"}，CUDA：${gpu?.cudaAvailable ? "可用" : "不可用"}`,
    `安装状态：${installation?.message ?? "未读取"}`,
    `已识别本地模型：${installation?.installedModels.length ?? 0} 个`,
  ];
  const verifyInstallation = async () => {
    setBusy(true);
    try {
      const status = await getInstallationStatus(draft);
      onInstallationChange(status);
      onNotify(status.healthy ? "模型与 ComfyUI 校验通过" : status.message, status.healthy ? "success" : "warning");
    } finally { setBusy(false); }
  };
  const savePaths = async () => { await onSettingsChange(draft); onNotify("模型路径设置已保存"); };
  const toggleModel = async (path: string) => {
    const enabled = enabledModelSet.has(path);
    const next = enabled ? enabledModels.filter((model) => model !== path) : [...enabledModels, path];
    const nextSettings = { ...settings, enabledModels: next };
    setDraft(nextSettings);
    await onSettingsChange(nextSettings);
    onNotify(enabled ? `已停用 ${basename(path)}` : `已启用 ${basename(path)}`);
  };
  const exportLogs = async () => {
    const target = await exportTextFile("MagineStudio-runtime.log", runtimeLogs.join("\n"));
    if (target) onNotify(`运行日志已导出：${target}`);
  };

  return (
    <section className="page page-models">
      <div className="model-system-cards">
        <SystemCard icon={Box} title="ComfyUI" value={`版本：${installation?.comfyVersion ?? "未安装"}`} state={comfy.state === "ready" ? "运行中" : installation?.installed ? "已安装" : "待安装"} action={comfy.state === "ready" ? "打开 ComfyUI" : "启动 ComfyUI"} onClick={() => comfy.state === "ready" ? openExternal(comfy.url) : installation?.installed ? toggleComfy() : onNeedInstall()} tone="blue" busy={busy} />
        <SystemCard icon={Sparkles} title="Python" value={installation?.runtimePath ? "独立运行环境" : "未配置"} state={installation?.installed ? "正常" : "待配置"} action="查看 Python 文档" onClick={() => openExternal("https://www.python.org/doc/")} tone="python" />
        <SystemCard icon={Zap} title="CUDA" value={`驱动：${gpu?.driverVersion ?? "未知"}`} state={gpu?.cudaAvailable ? "可用" : "未检测到"} action="查看驱动下载" onClick={() => openExternal("https://www.nvidia.com/Download/index.aspx?lang=cn")} tone="green" />
        <section className="storage-card glass-panel"><div className="storage-ring" style={{ "--used": `${allInstalledModels.length ? Math.round(enabledModels.length / allInstalledModels.length * 100) : 0}%` } as React.CSSProperties}><span>{enabledModels.length}/{allInstalledModels.length}</span></div><div><h3>本地模型文件</h3><p>已启用 {enabledModels.length} 个，支持同时启用</p><small>磁盘可用 {disk ? formatBytes(disk.availableBytes, 1) : "—"}</small></div></section>
      </div>

      <div className="models-layout">
        <section className="model-list-panel glass-panel">
          <div className="section-title"><h3>本地模型列表</h3><button onClick={verifyInstallation}><RefreshCw size={14} />重新校验</button></div>
          <div className="model-table-head"><span>模型名称</span><span>类型</span><span>版本</span><span>大小</span><span>状态</span><span>操作</span></div>
          {installedModels.map((path, index) => { const name = basename(path); const workflow = name.toLowerCase().endsWith(".json"); const enabled = enabledModelSet.has(path); return <div className={`model-row ${enabled ? "is-enabled" : ""}`} key={path}><span className="model-name"><i className={`model-cube cube-${index}`}>{workflow ? "WF" : "H3"}</i>{name}</span><span>{workflow ? "ComfyUI 工作流" : "本地模型文件"}</span><span>本地</span><span>已安装</span><strong className={enabled ? "enabled" : "available"}><i />{enabled ? "已启用" : "可用"}</strong><span className="model-actions"><button aria-pressed={enabled} onClick={() => toggleModel(path)}>{enabled ? "停用" : "启用"}</button><button onClick={onNeedInstall}>更新</button><button onClick={verifyInstallation}>校验</button></span></div>; })}
          {!installedModels.length && <PanelEmpty icon={Box} title={query ? "未找到匹配模型" : "暂无已安装模型"} description={query ? "请更换关键词，或重新校验模型目录。" : "这里不再展示演示模型；完成一键安装后仅列出实际文件。"} action={<button className="empty-action" onClick={query ? () => setQuery("") : onNeedInstall}>{query ? "清除搜索" : "前往一键安装"}</button>} />}
          <div className="model-list-footer"><span>共 {allInstalledModels.length} 项</span><span>{enabledModels.length ? `已同时启用 ${enabledModels.length} 项` : "未启用任何模型"}</span><button onClick={verifyInstallation}>批量校验 <CheckCircle2 size={14} /></button></div>
        </section>

        <div className="model-side-column">
          <aside className="download-panel glass-panel">
            <h3>模型仓库与下载管理</h3>
            <div className="repo-search"><Search size={16} /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="搜索本地模型名称..." /><select aria-label="模型来源"><option>本地模型</option></select></div>
            <PanelEmpty icon={Download} title="暂无进行中的真实下载" description="模型下载由一键安装页统一管理，进度会在安装时实时显示。" action={<button className="empty-action" onClick={onNeedInstall}>打开一键安装</button>} />
            <div className="download-footer"><span>下载队列为空</span><button onClick={verifyInstallation}>刷新状态</button></div>
          </aside>

          <div className="model-settings-row">
            <section className="model-paths glass-panel"><h3>路径设置</h3><SmallPath label="模型库路径" value={draft.modelPath} onPick={() => pick("modelPath")} /><SmallPath label="输出路径" value={draft.outputPath} onPick={() => pick("outputPath")} /><button className="apply-paths" onClick={savePaths}>保存路径设置</button></section>
          </div>
        </div>
      </div>

      <section className="runtime-log glass-panel"><div className="section-title"><h3>运行日志</h3><span><button onClick={() => { setShowLogs(false); onNotify("运行日志视图已清空", "info"); }}>清空</button><button onClick={exportLogs}>导出日志</button></span></div><div>{showLogs ? runtimeLogs.map((line) => <p key={line}><b>[INFO]</b>　{line}</p>) : <p>暂无新的运行日志。</p>}</div></section>
    </section>
  );
}

function SystemCard({ icon: Icon, title, value, state, action, tone, onClick, busy }: { icon: LucideIcon; title: string; value: string; state: string; action: string; tone: string; onClick: () => void; busy?: boolean }) {
  return <section className="system-card glass-panel"><div className={`system-logo ${tone}`}>{busy ? <LoaderCircle className="spin" size={36} /> : <Icon size={39} />}</div><div><h3>{title}</h3><p>{value}</p><small>状态：<strong>{state}</strong></small></div><button onClick={onClick}>{action}</button></section>;
}

function SmallPath({ label, value, onPick }: { label: string; value: string; onPick: () => void }) {
  return <label className="small-path"><span>{label}</span><input value={value} readOnly /><button onClick={onPick}>浏览</button></label>;
}

function ReferenceChip({ path, label, onRemove }: { path: string; label?: string; onRemove: () => void }) {
  const kind = referenceMediaKind(path);
  const Icon = kind === "video" ? Film : kind === "audio" ? AudioLines : FileImage;
  const kindLabel = kind === "video" ? "视频" : kind === "audio" ? "音频" : "图片";
  return <div className="frame-chip"><Icon size={16} /><strong>{label ?? kindLabel}</strong><span title={path}>{basename(path)}</span><button type="button" aria-label={`移除${label ?? kindLabel} ${basename(path)}`} onClick={onRemove}><X size={14} /></button></div>;
}

function CreatePage({ report, settings, installation, comfy, onComfyChange, lastJob, recentJobs, onJobChange, onDeleteRecentJob, onNeedInstall, onNotify }: {
  report: HardwareReport;
  settings: AppSettings;
  installation: InstallationStatus | null;
  comfy: ComfyStatus;
  onComfyChange: (status: ComfyStatus) => void;
  lastJob: GenerationJob | null;
  recentJobs: GenerationJob[];
  onJobChange: React.Dispatch<React.SetStateAction<GenerationJob | null>>;
  onDeleteRecentJob: (promptId: string) => void;
  onNeedInstall: () => void;
  onNotify: (message: string, tone?: "success" | "info" | "warning") => void;
}) {
  const initialDraft = useRef(loadGenerationDraft()).current;
  const [prompt, setPrompt] = useState(initialDraft.prompt ?? "");
  const [negative, setNegative] = useState(initialDraft.negative ?? "");
  const [mode, setMode] = useState<GenerationMode>(initialDraft.mode && ["text", "image", "frames", "reference"].includes(initialDraft.mode) ? initialDraft.mode : "text");
  const [queueTab, setQueueTab] = useState<"queue" | "recent">("queue");
  const [selectedTaskId, setSelectedTaskId] = useState<string | null>(null);
  const [firstFrame, setFirstFrame] = useState<string | null>(initialDraft.firstFrame ?? null);
  const [lastFrame, setLastFrame] = useState<string | null>(initialDraft.lastFrame ?? null);
  const [referencePaths, setReferencePaths] = useState<string[]>(Array.isArray(initialDraft.referencePaths) ? initialDraft.referencePaths : []);
  const installedModelPaths = installation?.installedModels ?? [];
  const enabledModelPaths = settings.enabledModels ?? installedModelPaths;
  const generationModels = useMemo(
    () => installedModelPaths.filter((path) => enabledModelPaths.includes(path) && isGenerationModel(path)),
    [enabledModelPaths, installedModelPaths],
  );
  const standardModels = useMemo(() => generationModels.filter((path) => !isReferenceModel(path)), [generationModels]);
  const referenceModels = useMemo(() => generationModels.filter(isReferenceModel), [generationModels]);
  const selectableGenerationModels = mode === "reference" ? referenceModels : standardModels;
  const hasReferenceMode = referenceModels.length > 0;
  const supportModelsReady = useMemo(() => {
    const names = enabledModelPaths.map((path) => basename(path).toLowerCase());
    return ["qwen3vl_32b_minimax_h3", "audio_vae", "video_vae"].every((part) => names.some((name) => name.includes(part)));
  }, [enabledModelPaths]);
  const [selectedModel, setSelectedModel] = useState(initialDraft.selectedModel ?? "");
  const [width, setWidth] = useState(Number.isFinite(initialDraft.width) ? initialDraft.width! : report.preset.width);
  const [height, setHeight] = useState(Number.isFinite(initialDraft.height) ? initialDraft.height! : report.preset.height);
  const [duration, setDuration] = useState(Number.isFinite(initialDraft.duration) ? initialDraft.duration! : report.preset.durationSeconds);
  const [fps, setFps] = useState(initialDraft.fps === 30 ? 30 : 24);
  // ponytail: keep advanced ComfyUI parameters automatic until a dedicated advanced mode is needed.
  const steps = report.preset.steps;
  const seed = -1;
  const [busy, setBusy] = useState(lastJob?.state === "running" || lastJob?.state === "queued");
  const [engineBusy, setEngineBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const [videoCurrentTime, setVideoCurrentTime] = useState(0);
  const [videoDuration, setVideoDuration] = useState(0);
  const [videoPlaying, setVideoPlaying] = useState(false);
  const [videoRate, setVideoRate] = useState(1);
  const [videoMuted, setVideoMuted] = useState(false);
  const [videoNaturalSize, setVideoNaturalSize] = useState({ width: 0, height: 0 });
  useEffect(() => {
    if (!selectableGenerationModels.includes(selectedModel)) setSelectedModel(selectableGenerationModels[0] ?? "");
  }, [selectableGenerationModels, selectedModel]);
  useEffect(() => {
    if (!lastJob || !["completed", "failed"].includes(lastJob.state)) return;
    setBusy(false);
    setSelectedTaskId(lastJob.promptId);
    if (lastJob.state === "completed") setQueueTab("recent");
  }, [lastJob]);
  useEffect(() => {
    if (GENERATION_RESOLUTIONS.some(([optionWidth, optionHeight]) => optionWidth === width && optionHeight === height)) return;
    setWidth(report.preset.width);
    setHeight(report.preset.height);
  }, [height, report.preset.height, report.preset.width, width]);
  useEffect(() => {
    const draft: GenerationDraft = { prompt, negative, mode, selectedModel, width, height, duration, fps, firstFrame, lastFrame, referencePaths };
    window.localStorage.setItem(GENERATION_DRAFT_KEY, JSON.stringify(draft));
  }, [duration, firstFrame, fps, height, lastFrame, mode, negative, prompt, referencePaths, selectedModel, width]);
  const activePromptId = lastJob && (lastJob.state === "queued" || lastJob.state === "running") ? lastJob.promptId : null;
  const activeClientId = activePromptId ? lastJob?.clientId ?? null : null;
  const previewSource = generationVideoSource(lastJob);
  useEffect(() => {
    setPreviewError(null);
    setVideoCurrentTime(0);
    setVideoDuration(0);
    setVideoPlaying(false);
    setVideoRate(1);
    setVideoNaturalSize({ width: 0, height: 0 });
  }, [previewSource]);

  useEffect(() => {
    if (!activePromptId || !activeClientId || !comfy.url) return;
    const websocketUrl = `${comfy.url.replace(/^http/i, "ws")}/ws?clientId=${encodeURIComponent(activeClientId)}`;
    const socket = new WebSocket(websocketUrl);
    socket.onopen = () => socket.send(JSON.stringify({ type: "feature_flags", data: { supports_preview_metadata: false } }));
    const updateProgress = (percent: number, currentStep: number | null, totalSteps: number | null, message: string) => {
      const now = Date.now();
      onJobChange((current) => {
        if (!current || current.promptId !== activePromptId || !["queued", "running"].includes(current.state)) return current;
        const startedAtMs = Date.parse(current.startedAt ?? new Date(now).toISOString());
        const elapsedSeconds = Math.max(0, Math.round((now - startedAtMs) / 1000));
        const progressPercent = Math.max(current.progressPercent ?? 0, Math.min(99, Math.round(percent)));
        const estimatedRemainingSeconds = progressPercent >= 10
          ? Math.max(1, Math.round(elapsedSeconds * (100 - progressPercent) / Math.max(1, progressPercent)))
          : current.estimatedRemainingSeconds ?? null;
        return { ...current, state: "running", message, progressPercent, currentStep, totalSteps, elapsedSeconds, estimatedRemainingSeconds };
      });
    };
    socket.onmessage = (event) => {
      if (typeof event.data !== "string") return;
      try {
        const message = JSON.parse(event.data) as { type?: string; data?: Record<string, unknown> };
        const data = message.data ?? {};
        if (data.prompt_id && data.prompt_id !== activePromptId) return;
        if (message.type === "progress") {
          const value = Number(data.value ?? 0);
          const max = Number(data.max ?? 0);
          if (max > 0) updateProgress(10 + (value / max) * 85, value, max, `正在采样生成画面与声音 · ${value}/${max} 步`);
          return;
        }
        if (message.type === "progress_state") {
          const nodes = Object.values((data.nodes ?? {}) as Record<string, { value?: number; max?: number; state?: string }>);
          const sampler = nodes.filter((node) => Number(node.max ?? 0) > 1).sort((a, b) => Number(b.max ?? 0) - Number(a.max ?? 0))[0];
          if (sampler) {
            const value = Number(sampler.value ?? 0);
            const max = Number(sampler.max ?? 0);
            updateProgress(10 + (value / max) * 85, value, max, `正在采样生成画面与声音 · ${value}/${max} 步`);
          }
          return;
        }
        if (message.type === "executing") {
          const node = String(data.node ?? "");
          const stage = generationStage(node);
          updateProgress(stage.percent, null, null, stage.message);
        }
      } catch {
        // ComfyUI 还会发送二进制预览帧；实时任务状态只处理 JSON 文本事件。
      }
    };
    return () => socket.close();
  }, [activeClientId, activePromptId, comfy.url, onJobChange]);

  useEffect(() => {
    if (!activePromptId) return;
    const timer = window.setInterval(() => {
      onJobChange((current) => {
        if (!current || current.promptId !== activePromptId || !["queued", "running"].includes(current.state)) return current;
        const elapsedSeconds = Math.max(0, Math.round((Date.now() - Date.parse(current.startedAt ?? new Date().toISOString())) / 1000));
        return {
          ...current,
          elapsedSeconds,
          estimatedRemainingSeconds: current.estimatedRemainingSeconds === null || current.estimatedRemainingSeconds === undefined
            ? current.estimatedRemainingSeconds
            : Math.max(0, current.estimatedRemainingSeconds - 1),
        };
      });
    }, 1000);
    return () => window.clearInterval(timer);
  }, [activePromptId, onJobChange]);

  const startEngine = async (): Promise<ComfyStatus | null> => {
    if (!installation?.installed) { onNeedInstall(); return null; }
    setEngineBusy(true); setError(null);
    try {
      const status = await startComfy(settings);
      onComfyChange(status);
      return status;
    } catch (reason) {
      setError(errorMessage(reason));
      return null;
    } finally { setEngineBusy(false); }
  };
  const changeMode = (nextMode: GenerationMode) => {
    setMode(nextMode);
    setError(null);
  };
  const selectPromptAttachment = async () => {
    if (mode === "image") {
      const [path] = await chooseImages(false);
      if (path) setFirstFrame(path);
      return;
    }
    if (mode === "frames") {
      const paths = await chooseImages(true);
      if (!paths.length) return;
      setFirstFrame(paths[0]);
      setLastFrame(paths[1] ?? null);
      if (paths.length < 2) onNotify("首尾帧模式需要同时选择两张图片", "warning");
      return;
    }
    if (mode === "reference") {
      const paths = await chooseReferenceMedia();
      if (!paths.length) return;
      const limits: Record<ReferenceMediaKind, number> = { image: 9, video: 3, audio: 3 };
      const counts: Record<ReferenceMediaKind, number> = { image: 0, video: 0, audio: 0 };
      const accepted = paths.filter((path) => {
        const kind = referenceMediaKind(path);
        if (counts[kind] >= limits[kind]) return false;
        counts[kind] += 1;
        return true;
      });
      setReferencePaths(accepted);
      if (accepted.length !== paths.length) onNotify("参考素材已按模型上限保留：9 张图片、3 个视频、3 个音频", "warning");
    }
  };
  const generate = async (queued = false) => {
    if (!installation?.installed) { onNeedInstall(); return; }
    if (!prompt.trim()) { setError("请先填写正向提示词。"); return; }
    if (!selectedModel) { setError(mode === "reference" ? "请先在模型管理中启用 MiniMax H3 Ref2VA 模型。" : "请先在模型管理中启用 MiniMax H3 FL2VA 模型。"); return; }
    if (!supportModelsReady) { setError("生成前需要同时启用文本编码器、视频 VAE 和音频 VAE。"); return; }
    if (mode === "image" && !firstFrame) { setError("图生视频需要先点击回形针选择一张图片。"); return; }
    if (mode === "frames" && (!firstFrame || !lastFrame)) { setError("首尾帧模式需要点击回形针选择首帧和尾帧两张图片。"); return; }
    if (mode === "reference" && !referencePaths.length) { setError("全模参考需要先点击回形针添加图片、视频或音频素材。"); return; }
    const generationStartedAt = new Date().toISOString();
    const preparingId = `preparing-${crypto.randomUUID()}`;
    const preparingJob: GenerationJob = {
      promptId: preparingId,
      clientId: "",
      state: "queued",
      message: "正在启动本地引擎并准备生成任务…",
      videoUrl: null,
      localVideoPath: null,
      filename: null,
      startedAt: generationStartedAt,
      progressPercent: 0,
      currentStep: null,
      totalSteps: steps,
      elapsedSeconds: 0,
      estimatedRemainingSeconds: estimateGenerationSeconds(width, height, duration, fps, steps, report.gpus[0]?.vramMb ?? null),
      selectedModel,
      width,
      height,
      durationSeconds: duration,
      fps,
    };
    setBusy(true); setError(null); setQueueTab("queue"); setSelectedTaskId(preparingId); onJobChange(preparingJob);
    try {
      const engine = await startEngine();
      if (!engine || engine.state !== "ready") {
        setBusy(false);
        onJobChange((current) => current?.promptId === preparingId ? { ...current, state: "failed", message: "本地引擎启动失败，请检查服务状态后重试。", estimatedRemainingSeconds: null } : current);
        return;
      }
      const request: GenerationRequest = {
        prompt: `${prompt.trim()}${negative.trim() ? `\nNegative prompt: ${negative.trim()}` : ""}`,
        selectedModel,
        width,
        height,
        durationSeconds: duration,
        fps,
        steps,
        seed,
        firstFramePath: mode === "image" || mode === "frames" ? firstFrame : null,
        lastFramePath: mode === "frames" ? lastFrame : null,
        referencePaths: mode === "reference" ? referencePaths : [],
        outputPrefix: "MagineStudio",
      };
      const job = await queueGeneration(request, settings);
      const trackedJob: GenerationJob = {
        ...job,
        localVideoPath: job.localVideoPath ?? null,
        startedAt: generationStartedAt,
        progressPercent: 0,
        currentStep: null,
        totalSteps: steps,
        elapsedSeconds: 0,
        estimatedRemainingSeconds: estimateGenerationSeconds(width, height, duration, fps, steps, report.gpus[0]?.vramMb ?? null),
        selectedModel,
        width,
        height,
        durationSeconds: duration,
        fps,
      };
      setSelectedTaskId(job.promptId); onJobChange(trackedJob); onNotify(queued ? "任务已加入生成队列" : "任务已提交，正在本地生成", "info");
    } catch (reason) {
      const message = errorMessage(reason);
      setError(message); setBusy(false);
      onJobChange((current) => current?.promptId === preparingId ? { ...current, state: "failed", message, estimatedRemainingSeconds: null } : current);
    }
  };
  const cancel = async () => { await cancelGeneration(); setBusy(false); onJobChange((current) => current ? { ...current, state: "failed", message: "任务已取消", estimatedRemainingSeconds: null } : current); onNotify("生成任务已取消", "warning"); };
  const savePreset = () => {
    localStorage.setItem("magine-generation-preset", JSON.stringify({ selectedModel, width, height, duration, fps, negative, mode }));
    onNotify("当前生成参数已保存为本地预设");
  };
  const toggleVideoPlayback = async () => {
    const video = videoRef.current;
    if (!video || !previewSource) return;
    try {
      if (video.paused || video.ended) await video.play();
      else video.pause();
    } catch (reason) {
      setPreviewError(`无法播放本地视频：${errorMessage(reason)}`);
    }
  };
  const seekVideo = (nextTime: number) => {
    const video = videoRef.current;
    if (!video || !Number.isFinite(nextTime)) return;
    video.currentTime = Math.max(0, Math.min(video.duration || 0, nextTime));
    setVideoCurrentTime(video.currentTime);
  };
  const toggleVideoMuted = () => {
    const video = videoRef.current;
    if (!video || !previewSource) return;
    const muted = !video.muted;
    video.muted = muted;
    setVideoMuted(muted);
  };
  const cyclePlaybackRate = () => {
    const video = videoRef.current;
    if (!video || !previewSource) return;
    const rates = [1, 1.25, 1.5, 2];
    const nextRate = rates[(rates.indexOf(videoRate) + 1) % rates.length];
    video.playbackRate = nextRate;
    setVideoRate(nextRate);
  };
  const enterVideoFullscreen = async () => {
    const video = videoRef.current;
    if (!video || !previewSource) return;
    try { await video.requestFullscreen(); }
    catch (reason) { setPreviewError(`无法进入全屏：${errorMessage(reason)}`); }
  };
  const queueJobs = lastJob && (lastJob.state === "queued" || lastJob.state === "running") ? [lastJob] : [];
  const visibleJobs = queueTab === "queue" ? queueJobs : recentJobs;
  useEffect(() => {
    if (visibleJobs.some((job) => job.promptId === selectedTaskId)) return;
    setSelectedTaskId(visibleJobs[0]?.promptId ?? null);
  }, [queueTab, selectedTaskId, visibleJobs]);
  const deleteSelectedTask = async () => {
    if (!selectedTaskId) { onNotify("请先选择需要删除的任务", "info"); return; }
    if (queueTab === "queue") {
      if (lastJob?.promptId === selectedTaskId && ["queued", "running"].includes(lastJob.state)) await cancelGeneration();
      if (lastJob?.promptId === selectedTaskId) onJobChange(null);
      setBusy(false);
      onNotify("已删除选中的队列任务", "info");
    } else {
      onDeleteRecentJob(selectedTaskId);
      onNotify("已删除选中的最近生成任务", "info");
    }
    setSelectedTaskId(null);
  };
  const generationActive = Boolean(lastJob && ["queued", "running"].includes(lastJob.state));
  const playbackProgress = previewSource && videoDuration > 0 ? (videoCurrentTime / videoDuration) * 100 : lastJob ? jobProgress(lastJob) : 0;
  const previewWidth = videoNaturalSize.width || width;
  const previewHeight = videoNaturalSize.height || height;
  const portraitPreview = previewHeight > previewWidth;
  const previewStyle = { "--preview-aspect": `${previewWidth} / ${previewHeight}` } as React.CSSProperties;

  return (
    <section className="page page-create">
      <Stepper steps={["扫描硬件", "匹配模型", "选择路径", "安装部署"]} active={installation?.installed ? 3 : 2} />
      {!installation?.installed && <div className="create-warning"><AlertCircle size={17} />当前尚未安装本地模型，参数可以预览；点击“开始生成”将前往一键安装。</div>}
      <div className="create-layout">
        <section className="generation-form glass-panel">
          <h3>生成输入</h3>
          <div className={`mode-tabs ${hasReferenceMode ? "has-reference" : ""}`}><button className={mode === "text" ? "active" : ""} aria-pressed={mode === "text"} onClick={() => changeMode("text")}>文生视频</button><button className={mode === "image" ? "active" : ""} aria-pressed={mode === "image"} onClick={() => changeMode("image")}>图生视频</button><button className={mode === "frames" ? "active" : ""} aria-pressed={mode === "frames"} onClick={() => changeMode("frames")}>首尾帧</button>{hasReferenceMode && <button className={mode === "reference" ? "active" : ""} aria-pressed={mode === "reference"} onClick={() => changeMode("reference")}>全模参考</button>}</div>
          <label className="generation-model-select"><span><Layers3 size={15} />生成模型</span><select value={selectedModel} onChange={(event) => setSelectedModel(event.target.value)} disabled={!selectableGenerationModels.length}><option value="" disabled>{selectableGenerationModels.length ? "选择已启用的模型" : mode === "reference" ? "没有已启用的 Ref2VA 模型" : "没有已启用的 FL2VA 模型"}</option>{selectableGenerationModels.map((path) => <option value={path} key={path}>{generationModelLabel(path)}</option>)}</select><small>{selectedModel ? mode === "reference" ? "Ref2VA 支持参考图片、视频及音频；提示词可使用 <Picture 1>、<Video 1>、<Audio 1>。" : `当前加载：${basename(selectedModel)}` : mode === "reference" ? "请前往模型管理启用 Ref2VA 全模态参考模型。" : "请前往模型管理启用 FL2VA 模型。"}</small></label>
          <label className="prompt-field"><span>正向提示词 <em>{prompt.length} / 2000</em></span><textarea value={prompt} maxLength={2000} onChange={(event) => setPrompt(event.target.value)} placeholder="描述场景、主体动作、镜头运动、光线与声音。" />{mode !== "text" && <button type="button" className="prompt-upload" aria-label={mode === "reference" ? "上传全模态参考素材" : mode === "frames" ? "上传首帧和尾帧" : "上传参考图片"} title={mode === "reference" ? "上传图片、视频或音频" : mode === "frames" ? "选择首帧和尾帧" : "选择参考图片"} onClick={() => void selectPromptAttachment()}><Paperclip size={17} /></button>}</label>
          <label className="prompt-field negative"><span>反向提示词（可选） <em>{negative.length} / 2000</em></span><input value={negative} maxLength={2000} onChange={(event) => setNegative(event.target.value)} placeholder="不希望出现在视频中的内容" /><button type="button" aria-label="清空反向提示词" onClick={() => setNegative("")}><X size={15} /></button></label>
          {mode === "image" && firstFrame && <ReferenceChip path={firstFrame} label="参考图" onRemove={() => setFirstFrame(null)} />}
          {mode === "frames" && <div className="reference-chip-list">{firstFrame && <ReferenceChip path={firstFrame} label="首帧" onRemove={() => setFirstFrame(null)} />}{lastFrame && <ReferenceChip path={lastFrame} label="尾帧" onRemove={() => setLastFrame(null)} />}</div>}
          {mode === "reference" && referencePaths.length > 0 && <div className="reference-chip-list">{referencePaths.map((path) => <ReferenceChip path={path} key={path} onRemove={() => setReferencePaths((current) => current.filter((item) => item !== path))} />)}</div>}
          <div className="parameter-grid">
            <SettingControl label="时长"><select value={duration} onChange={(event) => setDuration(Number(event.target.value))}>{Array.from({ length: 12 }, (_, index) => index + 4).map((value) => <option value={value} key={value}>{value} 秒</option>)}</select></SettingControl>
            <SettingControl label="分辨率"><select value={`${width}x${height}`} onChange={(event) => { const [w, h] = event.target.value.split("x").map(Number); setWidth(w); setHeight(h); }}>{GENERATION_RESOLUTIONS.map(([optionWidth, optionHeight]) => <option value={`${optionWidth}x${optionHeight}`} key={`${optionWidth}x${optionHeight}`}>{optionWidth} × {optionHeight}{optionWidth === report.preset.width && optionHeight === report.preset.height ? "（推荐）" : optionWidth === optionHeight ? "（方形）" : optionWidth > optionHeight ? "（横屏）" : "（竖屏）"}</option>)}</select></SettingControl>
            <SettingControl label="帧率"><select value={fps} onChange={(event) => setFps(Number(event.target.value))}><option value={24}>24 FPS</option><option value={30}>30 FPS</option></select></SettingControl>
          </div>
          <div className="generation-actions"><button className="button secondary" onClick={savePreset}>保存预设</button><button className="button secondary" onClick={() => generate(true)} disabled={busy || engineBusy}><Download size={17} />加入队列</button>{busy ? <button className="button danger" onClick={cancel}><Square size={16} />取消生成</button> : <button className="button primary" onClick={() => generate(false)} disabled={engineBusy}>{engineBusy ? <LoaderCircle className="spin" size={18} /> : <Rocket size={18} />}{!installation?.installed ? "前往安装" : "开始生成"}</button>}</div>
          {error && <div className="inline-error"><AlertCircle size={16} />{error}</div>}
          <section className="generation-queue embedded">
            <div className="queue-tabs"><button className={queueTab === "queue" ? "active" : ""} onClick={() => setQueueTab("queue")}>生成队列 ({queueJobs.length})</button><button className={queueTab === "recent" ? "active" : ""} onClick={() => setQueueTab("recent")}>最近生成 ({recentJobs.length})</button><button className="queue-clear" onClick={() => void deleteSelectedTask()} disabled={!selectedTaskId} aria-label="删除选中的任务" title="删除选中的任务"><Trash2 size={16} /></button></div>
            {visibleJobs.length ? <div className="queue-list">{visibleJobs.map((job, index) => <QueueJobRow key={job.promptId} job={job} index={index} selected={selectedTaskId === job.promptId} onSelect={() => setSelectedTaskId(job.promptId)} onOpen={() => openOutputFolder(settings.outputPath)} />)}</div> : <PanelEmpty icon={ListTodo} title={queueTab === "queue" ? "生成队列为空" : "暂无最近生成"} description={queueTab === "queue" ? "点击开始生成后，任务会立即显示在这里。" : "生成完成的真实任务会保留在这里。"} compact />}
          </section>
        </section>

        <section className={`generation-preview ${portraitPreview ? "portrait" : "landscape"}`} style={previewStyle}>
          <div className="preview-video glass-panel">
            {previewSource ? <div className="preview-player"><video ref={videoRef} key={previewSource} src={previewSource} preload="auto" playsInline muted={videoMuted} aria-label="生成的视频预览" onClick={() => void toggleVideoPlayback()} onDoubleClick={() => void enterVideoFullscreen()} onLoadedMetadata={(event) => { setVideoDuration(Number.isFinite(event.currentTarget.duration) ? event.currentTarget.duration : 0); setVideoNaturalSize({ width: event.currentTarget.videoWidth, height: event.currentTarget.videoHeight }); setPreviewError(null); }} onDurationChange={(event) => setVideoDuration(Number.isFinite(event.currentTarget.duration) ? event.currentTarget.duration : 0)} onTimeUpdate={(event) => setVideoCurrentTime(event.currentTarget.currentTime)} onPlay={() => setVideoPlaying(true)} onPause={() => setVideoPlaying(false)} onEnded={() => setVideoPlaying(false)} onRateChange={(event) => setVideoRate(event.currentTarget.playbackRate)} onVolumeChange={(event) => setVideoMuted(event.currentTarget.muted)} onError={() => setPreviewError("本地视频加载失败，可点击下方打开输出目录查看文件。")}/>{previewError && <div className="preview-error"><AlertCircle size={18} /><strong>无法显示视频预览</strong><span>{previewError}</span><button onClick={() => openOutputFolder(settings.outputPath)}>打开输出目录</button></div>}</div> : <div className="preview-art"><div className="preview-h3">H3</div><div className="preview-copy"><Sparkles size={22} /><strong>{lastJob?.state === "completed" ? "视频生成完成" : lastJob ? `${jobProgress(lastJob)}% · 正在生成视频` : "本地视频预览"}</strong><span>{lastJob?.message ?? "生成完成后将在这里播放"}</span></div></div>}
            <div className="video-controls"><button onClick={() => void toggleVideoPlayback()} disabled={!previewSource} aria-label={videoPlaying ? "暂停视频" : "播放视频"}>{videoPlaying ? <Pause size={18} fill="currentColor" /> : <Play size={18} fill="currentColor" />}</button><span>{previewSource ? `${formatTaskTime(videoCurrentTime)} / ${formatTaskTime(videoDuration)}` : generationActive ? `${jobProgress(lastJob!)}%` : `00:00 / 00:0${duration}`}</span><input type="range" min={0} max={previewSource ? Math.max(videoDuration, 0.01) : 100} step={previewSource ? 0.01 : 1} value={previewSource ? Math.min(videoCurrentTime, Math.max(videoDuration, 0.01)) : playbackProgress} onChange={(event) => seekVideo(Number(event.target.value))} disabled={!previewSource} style={{ "--video-progress": `${playbackProgress}%` } as React.CSSProperties} aria-label="视频播放进度" /><button onClick={toggleVideoMuted} disabled={!previewSource} aria-label={videoMuted ? "恢复声音" : "静音"} title={videoMuted ? "恢复声音" : "静音"}>{videoMuted ? <VolumeX size={17} /> : <Volume2 size={17} />}</button><button className="rate-button" onClick={cyclePlaybackRate} disabled={!previewSource} aria-label={`播放速度 ${videoRate} 倍`} title="切换播放速度"><Settings2 size={15} /><b>{videoRate}×</b></button><button onClick={() => void enterVideoFullscreen()} disabled={!previewSource} aria-label="全屏播放" title="全屏播放"><Maximize2 size={17} /></button></div>
          </div>
          <div className="resource-cards"><ResourceCard icon={Cpu} label="显存容量" value={report.gpus[0]?.vramMb ? `${(report.gpus[0].vramMb! / 1024).toFixed(1)} GB` : "未知"} sub="真实硬件" /><ResourceCard icon={HardDrive} label="输出盘可用" value={diskForPath(report.disks, settings.outputPath) ? formatBytes(diskForPath(report.disks, settings.outputPath)!.availableBytes, 1) : "未知"} sub={diskForPath(report.disks, settings.outputPath)?.mountPoint ?? "未识别盘符"} /><ResourceCard icon={Layers3} label="当前任务" value={busy ? "1" : "0"} sub="当前会话" /><ResourceCard icon={Clock3} label="生成进度" value={lastJob ? `${jobProgress(lastJob)}%` : "空闲"} percent={lastJob && ["queued", "running"].includes(lastJob.state) ? jobProgress(lastJob) : undefined} sub={lastJob ? jobEtaLabel(lastJob) : "实时状态"} /></div>
        </section>
      </div>
    </section>
  );
}

function SettingControl({ label, children }: { label: string; children: React.ReactNode }) { return <label className="setting-control"><span>{label}</span>{children}</label>; }
function ResourceCard({ icon: Icon, label, value, percent, sub }: { icon: LucideIcon; label: string; value: string; percent?: number; sub?: string }) { return <article className="resource-card glass-panel"><div><Icon size={16} /><span>{label}</span></div><strong>{value}</strong>{percent !== undefined && <i><b style={{ width: `${percent}%` }} /></i>}{sub && <small>{sub}</small>}</article>; }

function JobProgress({ job }: { job: GenerationJob }) {
  const progress = jobProgress(job);
  return <div className="job-progress"><div><strong>{progress}%</strong><span>{jobEtaLabel(job)}</span></div><i><b style={{ width: `${progress}%` }} /></i><small>{job.currentStep !== null && job.currentStep !== undefined && job.totalSteps ? `采样 ${job.currentStep}/${job.totalSteps} 步` : `已用时 ${formatTaskTime(job.elapsedSeconds ?? 0)}`}</small></div>;
}

function QueueJobRow({ job, index, selected, onSelect, onOpen }: { job: GenerationJob; index: number; selected: boolean; onSelect: () => void; onOpen: () => void }) {
  return <div className={`queue-row ${selected ? "selected" : ""}`}><label className="queue-radio" title="选择任务"><input type="radio" name="generation-task" checked={selected} onChange={onSelect} /><i /></label><TaskThumb index={index} /><div className="queue-job-info"><span><strong>{job.filename || "MiniMax H3 本地生成任务"}</strong><small>{job.message}</small></span><JobProgress job={job} /></div><div className="queue-job-actions"><em className={job.state === "running" ? "working" : "done"}>{TaskStateLabel(job.state)}</em><button onClick={onOpen} aria-label="打开输出目录"><FolderOpen size={15} /></button></div></div>;
}

function TasksPage({ job, settings, onJobChange, onCreate, onNotify }: { job: GenerationJob | null; settings: AppSettings; onJobChange: React.Dispatch<React.SetStateAction<GenerationJob | null>>; onCreate: () => void; onNotify: (message: string, tone?: "success" | "info" | "warning") => void }) {
  const [filter, setFilter] = useState<"all" | GenerationJob["state"]>("all");
  const [query, setQuery] = useState("");
  const [showDetail, setShowDetail] = useState(true);
  const matches = Boolean(job && (filter === "all" || job.state === filter) && (`${job.filename ?? ""} ${job.message}`).toLowerCase().includes(query.trim().toLowerCase()));
  const running = job?.state === "running" || job?.state === "queued";
  const stopJob = async () => {
    if (!running || !job) { onNotify("当前没有运行中的任务", "info"); return; }
    await cancelGeneration();
    onJobChange({ ...job, state: "failed", message: "任务已由用户停止" });
    onNotify("运行中的任务已停止", "warning");
  };
  const clearFinished = () => {
    if (job && (job.state === "completed" || job.state === "failed")) { onJobChange(null); onNotify("已清除当前会话的已结束任务"); }
    else onNotify("没有可清除的已结束任务", "info");
  };
  const copyText = async (text: string) => { await navigator.clipboard.writeText(text); onNotify("内容已复制到剪贴板"); };
  const counts = { all: job ? 1 : 0, queued: job?.state === "queued" ? 1 : 0, running: job?.state === "running" ? 1 : 0, completed: job?.state === "completed" ? 1 : 0, failed: job?.state === "failed" ? 1 : 0 };
  return (
    <section className="page page-tasks">
      <PageHeading title="任务队列" description="仅展示当前客户端会话中的真实生成任务" action={<div className="task-toolbar"><button onClick={clearFinished}><Trash2 size={16} />清空已结束</button><button onClick={stopJob} disabled={!running}><Pause size={16} />停止运行任务</button><button onClick={onCreate}><WandSparkles size={16} />新建任务</button></div>} />
      <div className="task-stats"><TaskStat label="运行中任务数" value={String(counts.running + counts.queued)} sub={running ? "任务正在本机处理" : "当前无运行任务"} tone="blue" /><TaskStat label="当前会话完成" value={String(counts.completed)} sub="来自真实生成结果" tone="green" /><TaskStat label="当前会话失败" value={String(counts.failed)} sub={job?.state === "failed" ? job.message : "无失败记录"} tone="red" /><TaskStat label="队列总数" value={String(counts.all)} sub="当前客户端会话" tone="purple" /></div>
      <div className="tasks-layout">
        <section className="tasks-table glass-panel">
          <div className="task-filter"><div><button className={filter === "all" ? "active" : ""} onClick={() => setFilter("all")}>全部 ({counts.all})</button><button className={filter === "running" ? "active" : ""} onClick={() => setFilter("running")}>运行中 ({counts.running})</button><button className={filter === "queued" ? "active" : ""} onClick={() => setFilter("queued")}>排队中 ({counts.queued})</button><button className={filter === "completed" ? "active" : ""} onClick={() => setFilter("completed")}>已完成 ({counts.completed})</button><button className={filter === "failed" ? "active" : ""} onClick={() => setFilter("failed")}>失败 ({counts.failed})</button></div><label><Search size={16} /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="搜索真实任务" /></label></div>
          <div className="tasks-head"><span /><span>缩略图</span><span>任务名称</span><span>类型</span><span>模型</span><span>分辨率</span><span>时长</span><span>创建时间</span><span>进度</span><span>状态</span><span>操作</span></div>
          {matches && job ? <button className="tasks-row selected" onClick={() => setShowDetail(true)}><span className="row-check"><Check size={13} /></span><TaskThumb index={0} /><span className="task-name"><strong>{job.filename || "MiniMax H3 本地生成任务"}</strong><small>任务ID: {job.promptId}</small></span><span>本地生成</span><span title={job.selectedModel}>{job.selectedModel ? basename(job.selectedModel) : "MiniMax H3"}</span><span>{job.width && job.height ? `${job.width}×${job.height}` : "—"}</span><span>{job.durationSeconds ? `${job.durationSeconds} 秒` : "—"}</span><span>{job.startedAt ? formatDate(job.startedAt) : "—"}</span><span className="table-progress"><strong>{jobProgress(job)}%</strong><i><b style={{ width: `${jobProgress(job)}%` }} /></i><small>{jobEtaLabel(job)}</small></span><TaskStatus state={job.state} /><span className="row-actions">{running ? <Pause size={15} /> : <FolderOpen size={15} />}</span></button> : <PanelEmpty icon={ListTodo} title={job ? "当前筛选下没有任务" : "暂无真实任务"} description={job ? "切换筛选条件或清除搜索词。" : "完成一次本地生成后，真实任务会出现在这里。"} action={<button className="empty-action" onClick={job ? () => { setFilter("all"); setQuery(""); } : onCreate}>{job ? "重置筛选" : "创建任务"}</button>} />}
          <div className="table-footer"><span>共 {matches ? 1 : 0} 条真实任务</span><span>数据范围：当前客户端会话</span></div>
        </section>
        <aside className="task-detail glass-panel">
          <div className="section-title"><h3>任务详情</h3><button className="icon-button" onClick={() => setShowDetail(false)} aria-label="收起任务详情"><X size={17} /></button></div>
          {job && showDetail ? <><div className="detail-title"><strong>{job.filename || "MiniMax H3 本地生成任务"}</strong><TaskStatus state={job.state} /></div><div className="task-detail-art thumb-0"><div>H3</div></div><div className="detail-progress"><div><span>生成进度</span><strong>{jobProgress(job)}%</strong></div><i><b style={{ width: `${jobProgress(job)}%` }} /></i><p><span>已用时 {formatTaskTime(job.elapsedSeconds ?? 0)}</span><span>{jobEtaLabel(job)}</span></p></div><DetailBlock title="任务消息" onCopy={() => copyText(job.message)}><p>{job.message}</p></DetailBlock><DetailBlock title="任务标识" onCopy={() => copyText(`${job.promptId}\n${job.clientId}`)}><dl><dt>Prompt ID</dt><dd>{job.promptId}</dd><dt>Client ID</dt><dd>{job.clientId}</dd><dt>模型</dt><dd>{job.selectedModel ? basename(job.selectedModel) : "MiniMax H3"}</dd><dt>分辨率</dt><dd>{job.width && job.height ? `${job.width}×${job.height}` : "—"}</dd><dt>时长 / 帧率</dt><dd>{job.durationSeconds && job.fps ? `${job.durationSeconds} 秒 / ${job.fps} FPS` : "—"}</dd><dt>状态</dt><dd>{TaskStateLabel(job.state)}</dd></dl></DetailBlock><DetailBlock title="输出信息" onCopy={() => copyText(job.localVideoPath ?? job.videoUrl ?? settings.outputPath)}><p className="detail-log">文件：{job.filename ?? "尚未生成文件"}<br />输出地址：{job.localVideoPath ?? job.videoUrl ?? settings.outputPath}</p></DetailBlock><div className="detail-output"><span>保存路径</span><strong title={settings.outputPath}>{settings.outputPath}</strong><span>当前状态</span><strong>{TaskStateLabel(job.state)}</strong></div><div className="detail-actions"><button onClick={() => openOutputFolder(settings.outputPath)}><FolderOpen size={17} />打开目录</button><button onClick={onCreate}><RefreshCw size={17} />重新创建</button><button className="danger" onClick={stopJob} disabled={!running}><Trash2 size={17} />停止任务</button><button className="primary" onClick={() => openOutputFolder(settings.outputPath)}><FolderOpen size={17} />查看视频</button></div></> : <PanelEmpty icon={Info} title="未选择任务" description="从左侧选择一条真实任务查看详情。" compact />}
        </aside>
      </div>
      <button className="floating-create" onClick={onCreate}><WandSparkles size={18} />新建生成任务</button>
    </section>
  );
}

function TaskStat({ label, value, sub, tone }: { label: string; value: string; sub: string; tone: string }) { return <article className={`task-stat ${tone} glass-panel`}><span>{label}</span><strong>{value}</strong><small>{sub}</small><Activity size={48} /></article>; }
function TaskStatus({ state }: { state: string }) { const label = state === "running" || state === "queued" ? "运行中" : state === "completed" ? "已完成" : state === "failed" ? "失败" : "已暂停"; return <span className={`task-status ${state}`}>{label}</span>; }
function DetailBlock({ title, children, onCopy }: { title: string; children: React.ReactNode; onCopy: () => void }) { return <section className="detail-block"><h4>{title}<button onClick={onCopy}>复制</button></h4>{children}</section>; }

function SettingsPage({ report, settings, onSettingsChange, onRescan, onNotify }: { report: HardwareReport; settings: AppSettings; onSettingsChange: (settings: AppSettings) => Promise<void>; onRescan: () => Promise<void>; onNotify: (message: string, tone?: "success" | "info" | "warning") => void }) {
  const [draft, setDraft] = useState(settings);
  const [saving, setSaving] = useState(false);
  const [checking, setChecking] = useState(false);
  const dirty = draft.comfyPath !== settings.comfyPath || draft.modelPath !== settings.modelPath || draft.outputPath !== settings.outputPath;
  const save = async () => {
    setSaving(true);
    try {
      await onSettingsChange(draft);
      onNotify("路径设置已保存，环境状态已刷新");
    } catch (reason) {
      onNotify(`设置保存失败：${errorMessage(reason)}`, "warning");
    } finally {
      setSaving(false);
    }
  };
  const reset = async () => {
    try {
      const defaults = await getDefaultSettings();
      setDraft((current) => ({
        ...current,
        comfyPath: defaults.comfyPath,
        modelPath: defaults.modelPath,
        outputPath: defaults.outputPath,
      }));
      onNotify("已载入当前用户的默认路径，点击保存后生效", "info");
    } catch (reason) {
      onNotify(`无法读取默认设置：${errorMessage(reason)}`, "warning");
    }
  };
  const pickPath = async (key: "comfyPath" | "modelPath" | "outputPath") => {
    const selected = await chooseDirectory(draft[key]);
    if (selected) setDraft((current) => ({ ...current, [key]: normalizeWindowsPath(selected) }));
  };
  const cleanCache = async () => {
    const cachePath = `${draft.comfyPath}\\.h3studio-cache`;
    if (!window.confirm(`确认清理缓存目录？\n${cachePath}`)) return;
    try { await clearCache(cachePath); onNotify("安装下载缓存已安全清理"); }
    catch (reason) { onNotify(errorMessage(reason), "warning"); }
  };
  const checkLocalState = async () => {
    setChecking(true);
    try { await onRescan(); onNotify("硬件、环境、模型与本地服务状态已刷新"); }
    catch (reason) { onNotify(`本地环境检测失败：${errorMessage(reason)}`, "warning"); }
    finally { setChecking(false); }
  };
  const exportConfig = async () => {
    const target = await exportTextFile("MagineStudio-settings.json", JSON.stringify(draft, null, 2));
    if (target) onNotify(`配置已导出：${target}`);
  };
  return (
    <section className="page page-settings">
      <PageHeading title="设置中心" description="管理真实生效的本地路径与存储配置" />
      <div className="settings-layout">
        <section className="settings-main glass-panel">
          <div className="settings-tabs"><button className="active" type="button">常规设置</button></div>
          <div className="settings-content">
            <h3>常规设置</h3>
            <div className="settings-form-grid">
              <label><span>ComfyUI 路径</span><div className="settings-path-control"><input value={draft.comfyPath} spellCheck={false} onChange={(event) => setDraft({ ...draft, comfyPath: event.target.value })} /><button type="button" onClick={() => void pickPath("comfyPath")}>浏览</button></div></label>
              <label><span>模型库路径</span><div className="settings-path-control"><input value={draft.modelPath} spellCheck={false} onChange={(event) => setDraft({ ...draft, modelPath: event.target.value })} /><button type="button" onClick={() => void pickPath("modelPath")}>浏览</button></div></label>
              <label><span>输出路径</span><div className="settings-path-control"><input value={draft.outputPath} spellCheck={false} onChange={(event) => setDraft({ ...draft, outputPath: event.target.value })} /><button type="button" onClick={() => void pickPath("outputPath")}>浏览</button></div></label>
              <label className="settings-inline"><span>安装缓存</span><small title={`${draft.comfyPath}\\.h3studio-cache`}>ComfyUI 安装包与依赖下载缓存</small><button type="button" onClick={() => void cleanCache()}>立即清理</button></label>
            </div>
          </div>
        </section>
        <aside className="settings-aside">
          <section className="about-panel glass-panel"><h3>关于软件</h3><img src={brandLogo} alt="MagineStudio 标志" /><h2>MagineStudio</h2><p>MiniMax H3 · ComfyUI 本地部署</p><dl><dt>版本号</dt><dd>v1.0.0</dd><dt>构建日期</dt><dd>2026-08-11</dd><dt>源码许可</dt><dd>Apache-2.0</dd><dt>模型许可</dt><dd className={settings.licenseAccepted ? "green" : ""}>{settings.licenseAccepted ? "已确认" : "未确认"}</dd><dt>安装状态</dt><dd>{settings.acceptedAt ? "已保存配置" : "待部署"}</dd></dl></section>
          <section className="quick-operations glass-panel"><h3>快速操作</h3><button onClick={() => openOutputFolder(settings.comfyPath)}><FolderOpen size={17} />打开 ComfyUI 目录</button><button onClick={() => openOutputFolder(settings.outputPath)}><FolderOpen size={17} />打开输出文件夹</button><button onClick={() => void checkLocalState()} disabled={checking}>{checking ? <LoaderCircle className="spin" size={17} /> : <RefreshCw size={17} />}重新检测本地环境</button><button onClick={() => void reset()}><RotateCcw size={17} />恢复默认路径</button></section>
          <section className="hardware-note glass-panel"><MonitorCog size={20} /><span>{report.gpus[0]?.name ?? "未识别显卡"}</span><strong>{report.tierLabel}</strong></section>
        </aside>
      </div>
      <div className="settings-actions"><button className="button secondary" onClick={exportConfig}><Upload size={18} />导出配置</button><button className="button secondary" onClick={() => void reset()}><RotateCcw size={18} />恢复默认路径</button><button className="button primary" onClick={() => void save()} disabled={!dirty || saving}>{saving ? <LoaderCircle className="spin" size={18} /> : <CheckCircle2 size={18} />}{saving ? "正在保存" : dirty ? "保存并应用" : "设置已保存"}</button></div>
    </section>
  );
}

function PanelEmpty({ icon: Icon, title, description, action, compact = false }: { icon: LucideIcon; title: string; description: string; action?: React.ReactNode; compact?: boolean }) {
  return <div className={`panel-empty ${compact ? "compact" : ""}`}><Icon size={compact ? 22 : 28} /><div><strong>{title}</strong><span>{description}</span></div>{action}</div>;
}

function TaskThumb({ index }: { index: number }) { return <div className={`task-thumb thumb-${index % 6}`}><Film size={15} /></div>; }
function TaskStateLabel(state: GenerationJob["state"]) { return state === "queued" ? "排队中" : state === "running" ? "生成中" : state === "completed" ? "已完成" : "失败"; }
function generationStage(node: string) {
  if (["1", "2", "3", "4"].includes(node)) return { percent: 4, message: "正在加载本地模型与编码器…" };
  if (["5", "6", "7", "8", "9"].includes(node)) return { percent: 9, message: "正在准备生成条件与采样器…" };
  if (node === "10") return { percent: 10, message: "正在采样生成画面与声音…" };
  if (node === "11" || node === "12") return { percent: 96, message: "正在解码视频与音频…" };
  if (node === "13") return { percent: 98, message: "正在合成视频与声音…" };
  if (node === "14") return { percent: 99, message: "正在保存生成文件…" };
  return { percent: 1, message: "正在初始化生成任务…" };
}

function estimateGenerationSeconds(width: number, height: number, duration: number, fps: number, steps: number, vramMb: number | null) {
  const pixelScale = (width * height) / (608 * 352);
  const durationScale = duration / 4;
  const frameRateScale = fps / 24;
  const stepScale = steps / 12;
  const vramGb = vramMb ? vramMb / 1024 : 16;
  const vramFactor = Math.max(0.75, 16 / Math.min(24, vramGb));
  return Math.max(60, Math.round(180 * pixelScale * durationScale * frameRateScale * stepScale * vramFactor));
}

function jobProgress(job: GenerationJob) {
  if (job.state === "completed") return 100;
  return Math.max(0, Math.min(99, Math.round(job.progressPercent ?? 0)));
}

function formatTaskTime(seconds: number) {
  const safeSeconds = Math.max(0, Math.round(Number.isFinite(seconds) ? seconds : 0));
  const hours = Math.floor(safeSeconds / 3600);
  const minutes = Math.floor((safeSeconds % 3600) / 60);
  const remainder = safeSeconds % 60;
  const pad = (value: number) => String(value).padStart(2, "0");
  return hours > 0 ? `${hours}:${pad(minutes)}:${pad(remainder)}` : `${pad(minutes)}:${pad(remainder)}`;
}

function jobEtaLabel(job: GenerationJob) {
  if (job.state === "completed") return "已完成";
  if (job.state === "failed") return "已停止";
  if (job.estimatedRemainingSeconds === null || job.estimatedRemainingSeconds === undefined) return "预计剩余：计算中";
  return `预计剩余 ${formatTaskTime(job.estimatedRemainingSeconds)}`;
}

function diskForPath(disks: HardwareReport["disks"], path: string) {
  const normalized = path.replaceAll("/", "\\").toLowerCase();
  return disks
    .filter((disk) => normalized.startsWith(disk.mountPoint.replaceAll("/", "\\").toLowerCase()))
    .sort((left, right) => right.mountPoint.length - left.mountPoint.length)[0];
}

export default App;
