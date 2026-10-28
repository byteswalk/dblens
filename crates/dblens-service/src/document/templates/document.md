{#- DBLens 文档的 markdown 模板。上下文见 M4c 设计文档第 4 节（DocContent）；
    t('键') 取固定文字，|v 把空值写成空，|cell 转义表格单元格。 -#}
# {{ info.title }}

{{ t('doc.dictionary') if kind == 'dictionary' else t('doc.design') }}

{% if info.version %}
- {{ t('doc.version') }}: {{ info.version }}
{% endif %}
{% if info.author %}
- {{ t('doc.author') }}: {{ info.author }}
{% endif %}
- {{ t('doc.generated') }}: {{ info.generated }}
- {{ t('doc.source') }}: {{ info.source }}
{% if kind == 'dictionary' and info.description %}

{{ info.description }}
{% endif %}
{% if kind == 'design' %}

## {{ t('doc.overview') }}
{% if info.description %}

{{ info.description }}
{% endif %}

| {{ t('doc.stats') }} | |
|---|---:|
| {{ t('doc.stat.schemas') }} | {{ stats.schemas }} |
| {{ t('doc.stat.tables') }} | {{ stats.tables }} |
| {{ t('doc.stat.columns') }} | {{ stats.columns }} |
| {{ t('doc.stat.foreign_keys') }} | {{ stats.foreign_keys }} |
| {{ t('doc.stat.logical_keys') }} | {{ stats.logical_keys }} |
{% if stats.views %}
| {{ t('doc.stat.views') }} | {{ stats.views }} |
{% endif %}
{% if diagrams %}

## {{ t('doc.diagrams') }}
{% for d in diagrams %}

### {{ d.title }}
{% if d.description %}

{{ d.description }}
{% endif %}

![{{ d.title }}]({{ d.src }})
{% if d.names_only %}

> {{ t('doc.names_only') }}
{% endif %}
{% endfor %}
{% endif %}
{% if relations %}

## {{ t('doc.relations') }}

| {{ t('doc.col.from') }} | {{ t('doc.col.index_columns') }} | {{ t('doc.col.to') }} | {{ t('doc.col.index_columns') }} | {{ t('doc.col.kind') }} | {{ t('doc.col.name') }} |
|---|---|---|---|---|---|
{% for r in relations %}
| {{ r.from_schema }}.{{ r.from_table|cell }} | {{ r.from_columns|cell }} | {{ r.to_schema }}.{{ r.to_table|cell }} | {{ r.to_columns|cell }} | {{ t('doc.kind.' ~ r.kind) }} | {{ r.name|cell }} |
{% endfor %}
{% endif %}
{% endif %}

## {{ t('doc.tables') }}
{% for s in schemas %}

### {{ s.name }}

| {{ t('doc.col.no') }} | {{ t('doc.col.table') }} | {{ t('doc.col.comment') }} | {{ t('doc.col.columns') }} |
|---:|---|---|---:|
{% for tb in s.tables %}
| {{ loop.index }} | [{{ tb.name|cell }}](#{{ tb.anchor }}) | {{ tb.comment|cell }} | {{ tb.columns|length }} |
{% endfor %}
{% endfor %}
{% for s in schemas if s.views %}
{% if loop.first %}

## {{ t('doc.views') }}
{% endif %}
{% for vw in s.views %}

### <a id="{{ vw.anchor }}"></a>{{ s.name }}.{{ vw.name }}
{% if vw.comment %}

{{ vw.comment }}
{% endif %}

| {{ t('doc.col.no') }} | {{ t('doc.col.column') }} | {{ t('doc.col.type') }} | {{ t('doc.col.comment') }} |
|---:|---|---|---|
{% for c in vw.columns %}
| {{ c.ordinal }} | {{ c.name|cell }} | {{ c.data_type|cell }} | {{ c.comment|cell }} |
{% endfor %}
{% if vw.definition %}

```sql
{{ vw.definition }}
```
{% endif %}
{% endfor %}
{% endfor %}

## {{ t('doc.details') }}
{% for s in schemas %}
{% for tb in s.tables %}

### <a id="{{ tb.anchor }}"></a>{{ s.name }}.{{ tb.name }}
{% if tb.comment %}

{{ tb.comment }}
{% endif %}
{% if tb.partition %}

{{ t('doc.partition') }}: {{ tb.partition }}
{% endif %}

| {{ t('doc.col.no') }} | {{ t('doc.col.column') }} | {{ t('doc.col.type') }} | {{ t('doc.col.nullable') }} | {{ t('doc.col.default') }} | {{ t('doc.col.flags') }} | {{ t('doc.col.comment') }} |
|---:|---|---|---|---|---|---|
{% for c in tb.columns %}
| {{ c.ordinal }} | {{ c.name|cell }} | {{ c.data_type|cell }} | {{ t('doc.yes') if c.nullable else t('doc.no') }} | {{ c.default|cell }} | {% for f in c.flags %}{{ t('doc.flag.' ~ f) }}{{ ', ' if not loop.last }}{% endfor %} | {{ c.comment|cell }} |
{% endfor %}
{% if tb.indexes %}

#### {{ t('doc.sec.indexes') }}

| {{ t('doc.col.name') }} | {{ t('doc.col.index_columns') }} | {{ t('doc.col.unique') }} | {{ t('doc.col.method') }} |
|---|---|---|---|
{% for i in tb.indexes %}
| {{ i.name|cell }} | {{ i.columns|cell }} | {{ t('doc.yes') if i.unique else t('doc.no') }} | {{ i.method|cell }} |
{% endfor %}
{% endif %}
{% if tb.constraints %}

#### {{ t('doc.sec.constraints') }}

| {{ t('doc.col.kind') }} | {{ t('doc.col.name') }} | {{ t('doc.col.detail') }} |
|---|---|---|
{% for k in tb.constraints %}
| {{ t('doc.kind.' ~ k.kind) }} | {{ k.name|cell }} | {{ k.detail|cell }} |
{% endfor %}
{% endif %}
{% for title, keys in [(t('doc.sec.foreign_keys'), tb.foreign_keys), (t('doc.sec.logical_keys'), tb.logical_keys)] if keys %}

#### {{ title }}

| {{ t('doc.col.name') }} | {{ t('doc.col.index_columns') }} | {{ t('doc.col.references') }} | {{ t('doc.col.on_delete') }} | {{ t('doc.col.on_update') }} | {{ t('doc.col.comment') }} |
|---|---|---|---|---|---|
{% for k in keys %}
| {{ k.name|cell }} | {{ k.columns|cell }} | {% if k.ref_anchor %}[{{ k.ref_schema }}.{{ k.ref_table|cell }}](#{{ k.ref_anchor }}){% else %}{{ k.ref_schema }}.{{ k.ref_table|cell }}{% endif %} ({{ k.ref_columns|cell }}) | {{ k.on_delete|cell }} | {{ k.on_update|cell }} | {{ k.comment|cell }} |
{% endfor %}
{% endfor %}
{% if tb.triggers %}

#### {{ t('doc.sec.triggers') }}

| {{ t('doc.col.name') }} | {{ t('doc.col.timing') }} | {{ t('doc.col.events') }} |
|---|---|---|
{% for g in tb.triggers %}
| {{ g.name|cell }} | {{ g.timing|cell }} | {{ g.events|cell }} |
{% endfor %}
{% endif %}
{% if tb.referenced_by %}

#### {{ t('doc.sec.referenced_by') }}

| {{ t('doc.col.table') }} | {{ t('doc.col.index_columns') }} | {{ t('doc.col.references') }} | {{ t('doc.col.kind') }} | {{ t('doc.col.name') }} |
|---|---|---|---|---|
{% for r in tb.referenced_by %}
| {% if r.anchor %}[{{ r.schema }}.{{ r.table|cell }}](#{{ r.anchor }}){% else %}{{ r.schema }}.{{ r.table|cell }}{% endif %} | {{ r.columns|cell }} | {{ r.ref_columns|cell }} | {{ t('doc.kind.' ~ r.kind) }} | {{ r.name|cell }} |
{% endfor %}
{% endif %}
{% endfor %}
{% endfor %}
{% if unreadable %}

## {{ t('doc.unreadable') }}

| {{ t('doc.col.schema') }} | {{ t('doc.col.table') }} | {{ t('doc.col.file') }} | {{ t('doc.col.error') }} |
|---|---|---|---|
{% for u in unreadable %}
| {{ u.schema|cell }} | {{ u.name|cell }} | {{ u.path|cell }} | {{ u.message|cell }} |
{% endfor %}
{% endif %}
