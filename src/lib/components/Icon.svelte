<script lang="ts" module>
  import Bold from "@lucide/svelte/icons/bold";
  import Bookmark from "@lucide/svelte/icons/bookmark";
  import Calendar from "@lucide/svelte/icons/calendar";
  import Flag from "@lucide/svelte/icons/flag";
  import Star from "@lucide/svelte/icons/star";
  import User from "@lucide/svelte/icons/user";
  import Bot from "@lucide/svelte/icons/bot";
  import BookOpen from "@lucide/svelte/icons/book-open";
  import Code from "@lucide/svelte/icons/code";
  import Heading from "@lucide/svelte/icons/heading";
  import Heading1 from "@lucide/svelte/icons/heading-1";
  import Heading2 from "@lucide/svelte/icons/heading-2";
  import Heading3 from "@lucide/svelte/icons/heading-3";
  import Heading4 from "@lucide/svelte/icons/heading-4";
  import Image from "@lucide/svelte/icons/image";
  import Italic from "@lucide/svelte/icons/italic";
  import Link from "@lucide/svelte/icons/link";
  import Link2Off from "@lucide/svelte/icons/link-2-off";
  import List from "@lucide/svelte/icons/list";
  import ListChecks from "@lucide/svelte/icons/list-checks";
  import ListOrdered from "@lucide/svelte/icons/list-ordered";
  import Pilcrow from "@lucide/svelte/icons/pilcrow";
  import Quote from "@lucide/svelte/icons/quote";
  import SquareCode from "@lucide/svelte/icons/square-code";
  import MessageSquare from "@lucide/svelte/icons/message-square";
  import Strikethrough from "@lucide/svelte/icons/strikethrough";
  import Table from "@lucide/svelte/icons/table";
  import Activity from "@lucide/svelte/icons/activity";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import ArrowRightLeft from "@lucide/svelte/icons/arrow-right-left";
  import AtSign from "@lucide/svelte/icons/at-sign";
  import Check from "@lucide/svelte/icons/check";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import ChevronUp from "@lucide/svelte/icons/chevron-up";
  import ChevronsLeft from "@lucide/svelte/icons/chevrons-left";
  import ChevronsRight from "@lucide/svelte/icons/chevrons-right";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import Command from "@lucide/svelte/icons/command";
  import Copy from "@lucide/svelte/icons/copy";
  import CornerDownRight from "@lucide/svelte/icons/corner-down-right";
  import Download from "@lucide/svelte/icons/download";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import Eye from "@lucide/svelte/icons/eye";
  import File from "@lucide/svelte/icons/file";
  import FileMinus from "@lucide/svelte/icons/file-minus";
  import FilePlus from "@lucide/svelte/icons/file-plus";
  import FileText from "@lucide/svelte/icons/file-text";
  import Files from "@lucide/svelte/icons/files";
  import Folder from "@lucide/svelte/icons/folder";
  import FolderCog from "@lucide/svelte/icons/folder-cog";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import FolderPlus from "@lucide/svelte/icons/folder-plus";
  import FolderSearch from "@lucide/svelte/icons/folder-search";
  import Funnel from "@lucide/svelte/icons/funnel";
  import GitBranch from "@lucide/svelte/icons/git-branch";
  import GitFork from "@lucide/svelte/icons/git-fork";
  import Hash from "@lucide/svelte/icons/hash";
  import Info from "@lucide/svelte/icons/info";
  import Layers from "@lucide/svelte/icons/layers";
  import Lightbulb from "@lucide/svelte/icons/lightbulb";
  import ListTree from "@lucide/svelte/icons/list-tree";
  import Lock from "@lucide/svelte/icons/lock";
  import Minus from "@lucide/svelte/icons/minus";
  import Monitor from "@lucide/svelte/icons/monitor";
  import Moon from "@lucide/svelte/icons/moon";
  import Music from "@lucide/svelte/icons/music";
  import MoveHorizontal from "@lucide/svelte/icons/move-horizontal";
  import Pencil from "@lucide/svelte/icons/pencil";
  import PanelLeft from "@lucide/svelte/icons/panel-left";
  import PanelRight from "@lucide/svelte/icons/panel-right";
  import Pin from "@lucide/svelte/icons/pin";
  import PinOff from "@lucide/svelte/icons/pin-off";
  import Plus from "@lucide/svelte/icons/plus";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Save from "@lucide/svelte/icons/save";
  import Search from "@lucide/svelte/icons/search";
  import Settings from "@lucide/svelte/icons/settings";
  import StickyNote from "@lucide/svelte/icons/sticky-note";
  import Sun from "@lucide/svelte/icons/sun";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import Trash from "@lucide/svelte/icons/trash";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import Undo2 from "@lucide/svelte/icons/undo-2";
  import Video from "@lucide/svelte/icons/video";
  import Volume from "@lucide/svelte/icons/volume-2";
  import VolumeX from "@lucide/svelte/icons/volume-x";
  import X from "@lucide/svelte/icons/x";
  import ZoomIn from "@lucide/svelte/icons/zoom-in";
  import ZoomOut from "@lucide/svelte/icons/zoom-out";
  import Scan from "@lucide/svelte/icons/scan";

  // The one icon set of the app (Lucide), registered by name so components stay tree-shakeable.
  export const icons = {
    bold: Bold,
    bookmark: Bookmark,
    calendar: Calendar,
    flag: Flag,
    star: Star,
    user: User,
    bot: Bot,
    "book-open": BookOpen,
    code: Code,
    heading: Heading,
    "heading-1": Heading1,
    "heading-2": Heading2,
    "heading-3": Heading3,
    "heading-4": Heading4,
    image: Image,
    italic: Italic,
    link: Link,
    "link-off": Link2Off,
    list: List,
    "list-checks": ListChecks,
    "list-ordered": ListOrdered,
    pilcrow: Pilcrow,
    quote: Quote,
    "square-code": SquareCode,
    comment: MessageSquare,
    strike: Strikethrough,
    table: Table,
    activity: Activity,
    "arrow-left": ArrowLeft,
    "arrow-right": ArrowRight,
    "arrow-right-left": ArrowRightLeft,
    at: AtSign,
    check: Check,
    "chevron-down": ChevronDown,
    "chevron-right": ChevronRight,
    "chevron-up": ChevronUp,
    "chevrons-left": ChevronsLeft,
    "chevrons-right": ChevronsRight,
    "circle-alert": CircleAlert,
    command: Command,
    copy: Copy,
    "corner-down-right": CornerDownRight,
    download: Download,
    more: Ellipsis,
    external: ExternalLink,
    eye: Eye,
    file: File,
    "file-minus": FileMinus,
    "file-plus": FilePlus,
    "file-text": FileText,
    files: Files,
    folder: Folder,
    "folder-cog": FolderCog,
    "folder-open": FolderOpen,
    "folder-plus": FolderPlus,
    "folder-search": FolderSearch,
    filter: Funnel,
    branch: GitBranch,
    fork: GitFork,
    hash: Hash,
    info: Info,
    layers: Layers,
    lightbulb: Lightbulb,
    outline: ListTree,
    lock: Lock,
    minus: Minus,
    monitor: Monitor,
    moon: Moon,
    music: Music,
    width: MoveHorizontal,
    pencil: Pencil,
    plus: Plus,
    "panel-left": PanelLeft,
    "panel-right": PanelRight,
    pin: Pin,
    "pin-off": PinOff,
    refresh: RefreshCw,
    restore: RotateCcw,
    save: Save,
    search: Search,
    settings: Settings,
    "sticky-note": StickyNote,
    sun: Sun,
    trash: Trash,
    alert: TriangleAlert,
    undo: Undo2,
    video: Video,
    volume: Volume,
    "volume-x": VolumeX,
    x: X,
    "zoom-in": ZoomIn,
    "zoom-out": ZoomOut,
    fit: Scan,
  } as const;

  export type IconName = keyof typeof icons;
</script>

<script lang="ts">
  type Props = { name: IconName; size?: "md" | "sm" | "xs"; class?: string };
  let { name, size = "md", class: extra = "" }: Props = $props();
  const Glyph = $derived(icons[name]);
</script>

<Glyph class={["icon", size !== "md" && `icon--${size}`, extra]} aria-hidden="true" />
