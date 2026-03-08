export type Severity = 'Error' | 'Warning' | 'Info' | 'Suggestion'
export type Category = 'Bug' | 'Security' | 'Performance' | 'Style' | 'Documentation' | 'BestPractice' | 'Maintainability' | 'Testing' | 'Architecture'
export type FixEffort = 'Low' | 'Medium' | 'High'
export type ReviewStatus = 'Pending' | 'Running' | 'Complete' | 'Failed'

export interface CodeSuggestion {
  original_code: string
  suggested_code: string
  explanation: string
  diff: string
}

export interface Comment {
  id: string
  file_path: string
  line_number: number
  content: string
  rule_id?: string
  severity: Severity
  category: Category
  suggestion?: string
  confidence: number
  code_suggestion?: CodeSuggestion
  tags: string[]
  fix_effort: FixEffort
}

export interface ReviewSummary {
  total_comments: number
  by_severity: Record<string, number>
  by_category: Record<string, number>
  critical_issues: number
  files_reviewed: number
  overall_score: number
  recommendations: string[]
}

export interface ReviewSession {
  id: string
  status: ReviewStatus
  diff_source: string
  started_at: string
  completed_at?: string
  comments: Comment[]
  summary?: ReviewSummary
  files_reviewed: number
  error?: string
  diff_content?: string
}

export interface StatusResponse {
  repo_path: string
  branch?: string
  model: string
  adapter?: string
  base_url?: string
  active_reviews: number
}

export interface DoctorModel {
  name: string
  size_mb: number
  quantization?: string
  family?: string
  parameter_size?: string
}

export interface DoctorResponse {
  config: {
    model: string
    adapter?: string
    base_url: string
    api_key_set: boolean
    context_window?: number
  }
  endpoint_reachable: boolean
  endpoint_type?: string
  models: DoctorModel[]
  recommended_model?: string
}

export interface StartReviewRequest {
  diff_source: 'head' | 'staged' | 'branch'
  base_branch?: string
}

// Parsed diff structures for the viewer
export interface DiffFile {
  path: string
  oldPath?: string
  hunks: DiffHunk[]
  status: 'added' | 'modified' | 'deleted' | 'renamed'
}

export interface DiffHunk {
  header: string
  oldStart: number
  oldCount: number
  newStart: number
  newCount: number
  lines: DiffLine[]
}

export interface DiffLine {
  type: 'add' | 'del' | 'context'
  content: string
  oldNumber?: number
  newNumber?: number
}
