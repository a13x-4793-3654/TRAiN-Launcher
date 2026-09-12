export type BackupScope = "settings" | "full";

export interface BackupProfile {
  id: string;
  name: string;
  minecraft_version: string;
  mod_loader: string | null;
  managed_mod_filenames: string[];
  managed_resource_pack_filenames: string[];
  enabled_resource_packs: string[];
  last_server_address: string | null;
}

export interface BackupInfo {
  id: string;
  profile: BackupProfile;
  scope: BackupScope;
  created_at: string;
  size_bytes: number;
  automatic: boolean;
}

export interface BackupList {
  backups: BackupInfo[];
  warnings: string[];
}

export interface RestoreResult {
  backup: BackupInfo;
  safety_backup: BackupInfo | null;
  warnings: string[];
}

export interface SettingsSource {
  id: string;
  name: string;
  minecraft_version: string;
  mod_loader: string | null;
  game_dir: string;
}

export interface SettingsSourceList {
  minecraft_version: string;
  sources: SettingsSource[];
  warnings: string[];
}

export interface ImportSettingsResult {
  imported_settings: number;
  safety_backup: BackupInfo | null;
}

export interface GameActivity {
  revision: number;
  running_games: number;
  file_operation: boolean;
}
