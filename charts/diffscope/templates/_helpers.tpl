{{/*
Expand the name of the chart.
*/}}
{{- define "diffscope.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
*/}}
{{- define "diffscope.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{/*
Create chart name and version as used by the chart label.
*/}}
{{- define "diffscope.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Common labels
*/}}
{{- define "diffscope.labels" -}}
helm.sh/chart: {{ include "diffscope.chart" . }}
{{ include "diffscope.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{/*
Selector labels
*/}}
{{- define "diffscope.selectorLabels" -}}
app.kubernetes.io/name: {{ include "diffscope.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
Create the name of the service account to use
*/}}
{{- define "diffscope.serviceAccountName" -}}
{{- if .Values.serviceAccount.create }}
{{- default (include "diffscope.fullname" .) .Values.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.serviceAccount.name }}
{{- end }}
{{- end }}

{{/*
Ollama fully qualified name
*/}}
{{- define "diffscope.ollamaFullname" -}}
{{- printf "%s-ollama" (include "diffscope.fullname" .) }}
{{- end }}

{{/*
Computed Ollama URL (used when ollama.enabled and no explicit baseUrl)
*/}}
{{- define "diffscope.ollamaUrl" -}}
{{- printf "http://%s:%d" (include "diffscope.ollamaFullname" .) (int .Values.ollama.port) }}
{{- end }}

{{/*
Resolve the secret name (created or existing)
*/}}
{{- define "diffscope.secretName" -}}
{{- if .Values.secrets.existingSecret }}
{{- .Values.secrets.existingSecret }}
{{- else }}
{{- include "diffscope.fullname" . }}
{{- end }}
{{- end }}
