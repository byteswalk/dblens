// MySQL 系（MySQL、MariaDB、TiDB、OceanBase、PolarDB）的语言服务，单独成块懒加载。

import { MySQL } from 'dt-sql-parser'

import { adapt } from './dtsql'

export const mysql = adapt(new MySQL())
