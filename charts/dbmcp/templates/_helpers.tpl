{{/*
Chart name, truncated to the DNS label limit.
*/}}
{{- define "dbmcp.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Fully qualified app name, truncated to the DNS label limit.
*/}}
{{- define "dbmcp.fullname" -}}
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
Chart name and version for the chart label.
*/}}
{{- define "dbmcp.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Common labels.
*/}}
{{- define "dbmcp.labels" -}}
helm.sh/chart: {{ include "dbmcp.chart" . }}
{{ include "dbmcp.selectorLabels" . }}
app.kubernetes.io/version: {{ .Values.image.tag | default .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{/*
Selector labels.
*/}}
{{- define "dbmcp.selectorLabels" -}}
app.kubernetes.io/name: {{ include "dbmcp.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
Service account name.
*/}}
{{- define "dbmcp.serviceAccountName" -}}
{{- if .Values.serviceAccount.create }}
{{- default (include "dbmcp.fullname" .) .Values.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.serviceAccount.name }}
{{- end }}
{{- end }}

{{/*
Name of the Secret holding the token and password.
*/}}
{{- define "dbmcp.secretName" -}}
{{- default (include "dbmcp.fullname" .) .Values.existingSecret.name }}
{{- end }}

{{/*
Comma-separated Host header values dbmcp accepts: Service DNS names, localhost, Ingress hosts, config.http.allowed_hosts.
*/}}
{{- define "dbmcp.allowedHosts" -}}
{{- $fullname := include "dbmcp.fullname" . }}
{{- $ns := .Release.Namespace }}
{{- $hosts := list "localhost" "127.0.0.1" "::1" $fullname (printf "%s.%s" $fullname $ns) (printf "%s.%s.svc" $fullname $ns) (printf "%s.%s.svc.cluster.local" $fullname $ns) }}
{{- if .Values.ingress.enabled }}
{{- range .Values.ingress.hosts }}
{{- /* dbmcp matches hosts exactly, so a wildcard host could never match. */}}
{{- if not (hasPrefix "*" .host) }}
{{- $hosts = append $hosts .host }}
{{- end }}
{{- end }}
{{- end }}
{{- $hosts = concat $hosts (dig "http" "allowed_hosts" (list) .Values.config) }}
{{- $hosts | uniq | join "," }}
{{- end }}
