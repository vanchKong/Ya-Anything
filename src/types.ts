export type FileStatus =
  | "pending"
  | "loading"
  | "compressed"
  | "best"
  | "restored"
  | "error";

export interface FileItem {
  id: string;
  path: string;
  name: string;
  originSize: number;
  outputSize: number;
  ratio: number;
  status: FileStatus;
  message: string;
  selected: boolean;
  /** 上次处理方式，用于再处理 */
  lastOp?: "compress" | "resize";
}

export interface CompressResult {
  path: string;
  name: string;
  originSize: number;
  outputSize: number;
  ratio: number;
  status: string;
  message: string;
}

export interface PreviewResult {
  mime: string;
  originBase64: string;
  compressedBase64: string;
}

export function formatSize(bytes: number): string {
  if (!bytes && bytes !== 0) return "-";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(2)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
}

export function createId(): string {
  return crypto.randomUUID();
}
